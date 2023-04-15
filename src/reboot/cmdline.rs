use clap::Parser;

/// The reboot command will reboot the camera
#[derive(Parser, Debug)]
pub struct Opt {
    /// The name of the camera to change the lights of. Must be a name in the config
    pub camera: String,
}
