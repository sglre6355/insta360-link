//! Prints what the first connected Link camera reports about itself.
//!
//! This only reads from the camera.

use insta360_link::{Camera, Error};

fn main() -> Result<(), Error> {
    let camera = Camera::open_first()?;
    let info = camera.info();

    println!("{} at {}", info.model, info.id);
    match camera.firmware_version() {
        Ok(version) => println!("firmware {version}"),
        Err(err) => return Err(err),
    }

    let state = camera.state()?;
    println!("{state:#?}");
    Ok(())
}
