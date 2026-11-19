//! AMD's display library: the card's own numbers, from the driver's own library.
//!
//! `atiadlxx.dll` ships with every Radeon driver, in `System32`. It is loaded at
//! run time — never linked, never redistributed — and everything in this module
//! follows two rules:
//!
//! 1. **Read only.** The library exposes setters (fan speed, clock overrides,
//!    PowerPlay). None of them is declared here, so this crate cannot change a
//!    card even by mistake.
//! 2. **No panic on a foreign call.** Every status is checked against `ADL_OK`
//!    before a value is used, and every failure becomes `None`.
//!
//! The handshake is the one the library demands on RDNA cards, where the older
//! `ADL2_OverdriveN_Temperature_Get` answers `-8` (not supported) and the
//! performance-log interface is the only way in:
//!
//! ```text
//! ADL2_Main_Control_Create         → a context for this process
//! ADL2_Adapter_*                   → which adapter is the card that is present
//! ADL2_Device_PMLog_Device_Create  → a logging device on that adapter
//! ADL2_Adapter_PMLog_Support_Get   → which sensors can be logged
//! ADL2_Adapter_PMLog_Start         → start logging those sensors
//! ADL2_New_QueryPMLogData_Get      → read the sensor table
//! ```
//!
//! The structs are transcribed from the documented C layout, field for field:
//! a single wrong field shifts every value after it, so they are kept verbatim
//! and `#[repr(C)]`.

use std::os::raw::c_void;

use libloading::Library;

use crate::{usable_celsius, usable_percent};

/// `ADL_OK`: the only status that means the answer can be used.
const ADL_OK: i32 = 0;

/// The vendor id a Radeon reports. ADL's own `iVendorID` field is unreliable —
/// some builds report `1002` where the card is `0x1002` and some report nothing
/// at all — so the adapter is recognised by its name as well.
const AMD_VENDOR: i32 = 0x1002;

/// How many sensors the library's tables hold.
const MAX_SENSORS: usize = 256;

/// The sensor id of the card's edge temperature, in whole degrees Celsius.
const SENSOR_TEMPERATURE_EDGE: usize = 8;

/// The sensor id of the graphics engine's activity, in percent.
const SENSOR_ACTIVITY_GFX: usize = 19;

/// How often the driver refreshes the logged sensors, in milliseconds.
///
/// The plugin samples every two seconds; asking for a faster refresh than the
/// driver keeps costs nothing and means a sample is never stale.
const SAMPLE_RATE_MS: u32 = 500;

/// A device the ADL library keeps for this process.
pub struct Gpu {
    /// The library, kept alive for as long as the context is.
    _library: Library,
    functions: Functions,
    context: *mut c_void,
    adapter: i32,
    device: u32,
    /// Whether the performance log is running, so `Drop` knows to stop it.
    logging: bool,
}

/// The entry points, resolved once.
struct Functions {
    main_control_destroy: unsafe extern "C" fn(*mut c_void) -> i32,
    device_pmlog_device_create: unsafe extern "C" fn(*mut c_void, i32, *mut u32) -> i32,
    device_pmlog_device_destroy: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    adapter_pmlog_support_get: unsafe extern "C" fn(*mut c_void, i32, *mut SupportInfo) -> i32,
    adapter_pmlog_start:
        unsafe extern "C" fn(*mut c_void, i32, *mut StartInput, *mut StartOutput, u32) -> i32,
    adapter_pmlog_stop: unsafe extern "C" fn(*mut c_void, i32, u32) -> i32,
    query_pmlog: unsafe extern "C" fn(*mut c_void, i32, *mut DataOutput) -> i32,
}

// The context is a process-wide handle the library owns and hands back; the
// struct only carries the number. The sampler thread is the one that uses it.
unsafe impl Send for Gpu {}

impl Gpu {
    /// Opens the library and starts logging the first present, active Radeon.
    ///
    /// `None` on any machine where that is not possible — no Radeon, no ADL, no
    /// permission — because a sensor that is missing is an absence, not a fault.
    pub fn open() -> Option<Self> {
        let library = Self::load()?;
        let functions = Functions::resolve(&library)?;

        let mut context: *mut c_void = std::ptr::null_mut();
        let create = Self::symbol::<unsafe extern "C" fn(Allocator, i32, *mut *mut c_void) -> i32>(
            &library,
            b"ADL2_Main_Control_Create\0",
        )?;
        // SAFETY: the callback is a plain function pointer with the convention
        // the header states, and `context` is a valid pointer to a null slot.
        let status = unsafe { create(allocate, 1, &mut context) };
        if status != ADL_OK || context.is_null() {
            return None;
        }

        let adapter = Self::active_adapter(&library, context)?;
        let mut device: u32 = 0;
        // SAFETY: the context came from `Main_Control_Create` above.
        let status =
            unsafe { (functions.device_pmlog_device_create)(context, adapter, &mut device) };
        if status != ADL_OK {
            // SAFETY: the context is ours and still alive.
            unsafe { (functions.main_control_destroy)(context) };
            return None;
        }

        let mut gpu = Self {
            _library: library,
            functions,
            context,
            adapter,
            device,
            logging: false,
        };
        gpu.logging = gpu.start_logging();
        Some(gpu)
    }

    /// The card's edge temperature, in whole degrees Celsius.
    pub fn temperature(&self) -> Option<f32> {
        usable_celsius(self.sensor(SENSOR_TEMPERATURE_EDGE)?)
    }

    /// How busy the graphics engine is, in percent.
    pub fn activity(&self) -> Option<f32> {
        usable_percent(self.sensor(SENSOR_ACTIVITY_GFX)?)
    }

    /// One sensor's raw value, when the driver says it is supported.
    fn sensor(&self, id: usize) -> Option<i32> {
        if !self.logging {
            return None;
        }
        let mut data = DataOutput::new();
        // SAFETY: the context and the adapter come from this struct's own
        // construction, and `data` is a live, correctly sized table.
        let status = unsafe { (self.functions.query_pmlog)(self.context, self.adapter, &mut data) };
        if status != ADL_OK {
            return None;
        }
        let reading = data.sensors.get(id)?;
        (reading.supported != 0).then_some(reading.value)
    }

    /// Starts the performance log with every sensor the driver offers.
    fn start_logging(&mut self) -> bool {
        let mut support = SupportInfo::new();
        // SAFETY: both pointers belong to this struct's live construction.
        let status = unsafe {
            (self.functions.adapter_pmlog_support_get)(self.context, self.adapter, &mut support)
        };
        if status != ADL_OK {
            return false;
        }

        let mut input = StartInput::new();
        for (index, sensor) in support.sensors.iter().enumerate() {
            input.sensors[index] = *sensor;
        }
        input.sample_rate = SAMPLE_RATE_MS;
        let mut output = StartOutput::new();

        // SAFETY: same as above; `input` lists only sensor ids the driver just
        // said it supports.
        let status = unsafe {
            (self.functions.adapter_pmlog_start)(
                self.context,
                self.adapter,
                &mut input,
                &mut output,
                self.device,
            )
        };
        status == ADL_OK
    }

    /// The index of the first adapter that is present and in use.
    ///
    /// The library lists every display adapter on the machine, including
    /// inactive ones and the Microsoft renderer, so the card is chosen by the
    /// only two facts that matter: it exists, and it is the one driving the
    /// display.
    fn active_adapter(library: &Library, context: *mut c_void) -> Option<i32> {
        let number: unsafe extern "C" fn(*mut c_void, *mut i32) -> i32 =
            Self::symbol(library, b"ADL2_Adapter_NumberOfAdapters_Get\0")?;
        let info: unsafe extern "C" fn(*mut c_void, *mut AdapterInfo, i32) -> i32 =
            Self::symbol(library, b"ADL2_Adapter_AdapterInfo_Get\0")?;
        let active: unsafe extern "C" fn(*mut c_void, i32, *mut i32) -> i32 =
            Self::symbol(library, b"ADL2_Adapter_Active_Get\0")?;

        let mut count: i32 = 0;
        // SAFETY: the context is live and `count` is a valid out pointer.
        if unsafe { number(context, &mut count) } != ADL_OK || count <= 0 {
            return None;
        }
        let mut adapters = vec![AdapterInfo::new(); count as usize];
        let size = std::mem::size_of::<AdapterInfo>();
        // SAFETY: the vector holds `count` initialised, correctly sized entries.
        let status = unsafe { info(context, adapters.as_mut_ptr(), size as i32 * count) };
        if status != ADL_OK {
            return None;
        }

        adapters
            .iter()
            .filter(|adapter| adapter.present != 0 && adapter.exist != 0)
            .find(|adapter| {
                let mut is_active: i32 = 0;
                // SAFETY: same context; `is_active` is a valid out pointer.
                let status = unsafe { active(context, adapter.index, &mut is_active) };
                status == ADL_OK && is_active != 0 && Self::is_amd(adapter)
            })
            .map(|adapter| adapter.index)
    }

    /// Whether the adapter's own description says it is an AMD card.
    fn is_amd(adapter: &AdapterInfo) -> bool {
        let name = adapter.name_text().to_ascii_lowercase();
        adapter.vendor == AMD_VENDOR
            || adapter.vendor == 1002
            || name.contains("radeon")
            || name.contains("amd")
    }

    /// Loads the library AMD's driver installs.
    fn load() -> Option<Library> {
        // The system copy first: that is the one the driver keeps current, and
        // the only one a machine can be expected to have.
        // SAFETY: loading a library is unsafe only in that its initialisers run;
        // this one is AMD's own, shipped and signed with the driver.
        unsafe { Library::new("atiadlxx.dll") }.ok()
    }

    /// One entry point, or `None` when this driver does not export it.
    fn symbol<T>(library: &Library, name: &[u8]) -> Option<T>
    where
        T: Copy,
    {
        // SAFETY: `name` is a NUL-terminated name, and the caller's type is the
        // signature the header states for that name.
        unsafe { library.get::<T>(name).ok().map(|symbol| *symbol) }
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: every handle here came from this struct's own construction and
        // is destroyed exactly once.
        unsafe {
            if self.logging {
                (self.functions.adapter_pmlog_stop)(self.context, self.adapter, self.device);
            }
            (self.functions.device_pmlog_device_destroy)(self.context, self.device);
            (self.functions.main_control_destroy)(self.context);
        }
    }
}

impl Functions {
    /// Resolves every entry point this crate uses.
    fn resolve(library: &Library) -> Option<Self> {
        Some(Self {
            main_control_destroy: Gpu::symbol(library, b"ADL2_Main_Control_Destroy\0")?,
            device_pmlog_device_create: Gpu::symbol(library, b"ADL2_Device_PMLog_Device_Create\0")?,
            device_pmlog_device_destroy: Gpu::symbol(
                library,
                b"ADL2_Device_PMLog_Device_Destroy\0",
            )?,
            adapter_pmlog_support_get: Gpu::symbol(library, b"ADL2_Adapter_PMLog_Support_Get\0")?,
            adapter_pmlog_start: Gpu::symbol(library, b"ADL2_Adapter_PMLog_Start\0")?,
            adapter_pmlog_stop: Gpu::symbol(library, b"ADL2_Adapter_PMLog_Stop\0")?,
            query_pmlog: Gpu::symbol(library, b"ADL2_New_QueryPMLogData_Get\0")?,
        })
    }
}

/// The allocator the library asks for when it builds its own tables.
///
/// It keeps the memory for the life of the context and does not free it, so the
/// allocations live as long as the process — a handful of small tables created
/// once, which is what the header's own sample does.
extern "system" fn allocate(size: i32) -> *mut c_void {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let layout = match std::alloc::Layout::from_size_align(size as usize, 16) {
        Ok(layout) => layout,
        Err(_) => return std::ptr::null_mut(),
    };
    // SAFETY: the layout has a non-zero size by the check above.
    unsafe { std::alloc::alloc(layout) as *mut c_void }
}

/// The allocator's signature, for the constructor's type.
type Allocator = extern "system" fn(i32) -> *mut c_void;

/// `ADLAdapterInfo`: one display adapter, as the header lays it out.
#[repr(C)]
#[derive(Clone, Copy)]
struct AdapterInfo {
    size: i32,
    index: i32,
    udid: [u8; 256],
    bus: i32,
    device: i32,
    function: i32,
    vendor: i32,
    adapter_name: [u8; 256],
    display_name: [u8; 256],
    present: i32,
    exist: i32,
    driver_path: [u8; 256],
    driver_path_ext: [u8; 256],
    pnp_string: [u8; 256],
    os_display_index: i32,
}

impl AdapterInfo {
    /// A zeroed adapter with its own size stated, which the library requires.
    fn new() -> Self {
        let mut adapter = Self {
            size: std::mem::size_of::<Self>() as i32,
            index: 0,
            udid: [0; 256],
            bus: 0,
            device: 0,
            function: 0,
            vendor: 0,
            adapter_name: [0; 256],
            display_name: [0; 256],
            present: 0,
            exist: 0,
            driver_path: [0; 256],
            driver_path_ext: [0; 256],
            pnp_string: [0; 256],
            os_display_index: 0,
        };
        adapter.size = std::mem::size_of::<Self>() as i32;
        adapter
    }

    /// The adapter's name, as far as it is text.
    fn name_text(&self) -> String {
        let end = self
            .adapter_name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(self.adapter_name.len());
        String::from_utf8_lossy(&self.adapter_name[..end]).to_string()
    }
}

/// `ADLPMLogSupportInfo`: which sensors the driver can log.
#[repr(C)]
struct SupportInfo {
    sensors: [u16; MAX_SENSORS],
    reserved: [i32; 16],
}

impl SupportInfo {
    fn new() -> Self {
        Self {
            sensors: [0; MAX_SENSORS],
            reserved: [0; 16],
        }
    }
}

/// `ADLPMLogStartInput`: the sensors to log, and how often.
#[repr(C)]
struct StartInput {
    sensors: [u16; MAX_SENSORS],
    sample_rate: u32,
    reserved: [i32; 15],
}

impl StartInput {
    /// The list is terminated by a zero, which the driver reads as "no more".
    fn new() -> Self {
        Self {
            sensors: [0; MAX_SENSORS],
            sample_rate: 0,
            reserved: [0; 15],
        }
    }
}

/// `ADLPMLogStartOutput`: where the driver put the logging buffer.
#[repr(C)]
struct StartOutput {
    address: *mut c_void,
}

impl StartOutput {
    fn new() -> Self {
        Self {
            address: std::ptr::null_mut(),
        }
    }
}

/// `ADLSingleSensorData`: one logged sensor.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SensorData {
    supported: i32,
    value: i32,
}

/// `ADLPMLogDataOutput`: the whole table, one entry per sensor id.
#[repr(C)]
struct DataOutput {
    size: i32,
    sensors: [SensorData; MAX_SENSORS],
}

impl DataOutput {
    fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as i32,
            sensors: [SensorData::default(); MAX_SENSORS],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_adapter_layout_is_the_size_the_library_expects() {
        // 4 + 4 + 256 + 4*4 + 256 + 256 + 4 + 4 + 256 + 256 + 256 + 4,
        // with the two alignment gaps a C compiler inserts before the name
        // arrays. If this ever changes, every field after it is garbage.
        assert_eq!(std::mem::size_of::<AdapterInfo>(), 1572);
        assert_eq!(std::mem::align_of::<AdapterInfo>(), 4);
    }

    #[test]
    fn the_sensor_table_holds_two_hundred_and_fifty_six_entries() {
        assert_eq!(std::mem::size_of::<DataOutput>(), 4 + MAX_SENSORS * 8);
        assert_eq!(std::mem::size_of::<SupportInfo>(), MAX_SENSORS * 2 + 16 * 4);
        assert_eq!(
            std::mem::size_of::<StartInput>(),
            MAX_SENSORS * 2 + 4 + 15 * 4
        );
    }

    #[test]
    fn a_card_is_recognised_by_its_name_when_the_vendor_field_lies() {
        let mut adapter = AdapterInfo::new();
        adapter.vendor = 1002;
        let name = b"AMD Radeon RX 6600";
        adapter.adapter_name[..name.len()].copy_from_slice(name);
        assert!(Gpu::is_amd(&adapter));

        let mut intel = AdapterInfo::new();
        intel.vendor = 32902;
        let name = b"Intel(R) UHD 630";
        intel.adapter_name[..name.len()].copy_from_slice(name);
        assert!(!Gpu::is_amd(&intel));
    }

    #[test]
    fn the_allocator_refuses_a_size_it_cannot_serve() {
        assert!(allocate(0).is_null());
        assert!(allocate(-1).is_null());
        assert!(!allocate(64).is_null());
    }

    #[test]
    #[ignore = "asks the graphics driver on this machine"]
    fn live_temperature_and_activity() {
        let gpu = Gpu::open().expect("a Radeon with ADL installed");
        let temperature = gpu.temperature();
        let activity = gpu.activity();
        println!("LIVE gpu temperature={temperature:?} activity={activity:?}");
        // A card without a thermal sensor is a valid machine; a value outside a
        // card's range is not, and a panic is never.
        if let Some(value) = temperature {
            assert!((0.0..=150.0).contains(&value), "{value} is not a card");
        }
        if let Some(value) = activity {
            assert!((0.0..=100.0).contains(&value), "{value} is not a share");
        }
    }
}
