//! Prints what the first connected Link camera reports about itself.
//!
//! This only reads from the camera.

use insta360_link::{Camera, Error};

fn main() -> Result<(), Error> {
    let camera = Camera::open_first()?;
    let info = camera.info();

    println!("{} at {}", info.model, info.id);
    println!("firmware {}", camera.firmware_version()?);

    let state = camera.state()?;
    println!("{state:#?}");
    Ok(())
}
