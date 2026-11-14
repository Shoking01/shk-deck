//! What one key shows on the deck.
//!
//! A key shows exactly one thing, and this enum is that one thing. Keeping it as
//! a single map — rather than a colour map plus an image map that can disagree —
//! is what lets a third rendering arrive later without a migration: a generated
//! tile (a clock, a meter) becomes one more variant, not a second field.
//!
//! The engine stores *what* a key shows, never the bytes. Turning an
//! [`Appearance`] into a payload is the renderer's job, because only the renderer
//! knows the deck's image size, byte limit and encoding.
//!
//! ## Example
//!
//! ```
//! use ss550_engine::{Appearance, AssetName};
//!
//! let solid = Appearance::solid([0xDC, 0x26, 0x26]);
//! assert_eq!(solid.rgb(), Some([0xDC, 0x26, 0x26]));
//!
//! let image = Appearance::image(AssetName::parse("live.png")?);
//! assert_eq!(image.asset().map(AssetName::as_str), Some("live.png"));
//! assert_eq!(image.rgb(), None);
//! # Ok::<(), ss550_engine::AssetError>(())
//! ```

use serde::{Deserialize, Serialize};

use crate::assets::AssetName;

/// What one key shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Appearance {
    /// One flat colour over the whole tile.
    Solid {
        /// The colour, as 8-bit RGB.
        rgb: [u8; 3],
    },
    /// An image from the asset library.
    Image {
        /// The asset's name inside the library, never a path.
        asset: AssetName,
    },
}

impl Appearance {
    /// A key filled with one flat colour.
    pub fn solid(rgb: [u8; 3]) -> Self {
        Self::Solid { rgb }
    }

    /// A key showing an image from the library.
    pub fn image(asset: AssetName) -> Self {
        Self::Image { asset }
    }

    /// The flat colour, when this key is one.
    pub fn rgb(&self) -> Option<[u8; 3]> {
        match self {
            Self::Solid { rgb } => Some(*rgb),
            Self::Image { .. } => None,
        }
    }

    /// The asset this key names, when it shows an image.
    pub fn asset(&self) -> Option<&AssetName> {
        match self {
            Self::Solid { .. } => None,
            Self::Image { asset } => Some(asset),
        }
    }

    /// How the window describes this key in one short phrase.
    pub fn describe(&self) -> String {
        match self {
            Self::Solid { rgb } => format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]),
            Self::Image { asset } => asset.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> AssetName {
        AssetName::parse("live.png").expect("a valid asset name")
    }

    #[test]
    fn a_solid_appearance_carries_its_colour_and_no_asset() {
        let appearance = Appearance::solid([1, 2, 3]);
        assert_eq!(appearance.rgb(), Some([1, 2, 3]));
        assert_eq!(appearance.asset(), None);
        assert_eq!(appearance.describe(), "#010203");
    }

    #[test]
    fn an_image_appearance_carries_its_asset_and_no_colour() {
        let appearance = Appearance::image(asset());
        assert_eq!(appearance.rgb(), None);
        assert_eq!(appearance.asset().map(AssetName::as_str), Some("live.png"));
        assert_eq!(appearance.describe(), "live.png");
    }

    #[test]
    fn a_solid_appearance_round_trips_through_json() {
        let json =
            serde_json::to_string(&Appearance::solid([0xDC, 0x26, 0x26])).expect("serialise");
        assert!(json.contains("\"kind\":\"solid\""), "{json}");

        let restored: Appearance = serde_json::from_str(&json).expect("parse");
        assert_eq!(restored, Appearance::solid([0xDC, 0x26, 0x26]));
    }

    #[test]
    fn an_image_appearance_round_trips_through_json() {
        let json = serde_json::to_string(&Appearance::image(asset())).expect("serialise");
        assert!(json.contains("\"kind\":\"image\""), "{json}");
        assert!(json.contains("\"asset\":\"live.png\""), "{json}");

        let restored: Appearance = serde_json::from_str(&json).expect("parse");
        assert_eq!(restored, Appearance::image(asset()));
    }

    #[test]
    fn an_image_appearance_with_an_escaping_name_is_refused_on_load() {
        let json = r#"{"kind":"image","asset":"../escape.png"}"#;
        assert!(serde_json::from_str::<Appearance>(json).is_err());
    }
}
