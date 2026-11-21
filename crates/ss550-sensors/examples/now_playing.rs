//! Prints what Windows says is playing: the title and the artist.
//!
//! Support tool, not part of the window: when a track key shows nothing, this
//! answers whether the machine has a media session at all, before anything in the
//! deck is blamed.
//!
//! ```text
//! cargo run -p ss550-sensors --example now_playing
//! ```
//!
//! `no session` is a real answer: Windows keeps one per player, and a player that
//! is closed — or that has never played — has none.

fn main() {
    match ss550_sensors::media::now_playing() {
        Some(playing) => {
            println!("title:  {}", playing.title);
            println!("artist: {}", playing.artist);
            match &playing.art {
                Some(art) => println!("cover:  {} bytes", art.len()),
                None => println!("cover:  none"),
            }
        }
        None => println!("no session: nothing on this machine reports a current track"),
    }
}
