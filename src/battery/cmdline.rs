use clap::Parser;

/// The battery command will dump the battery status to XML
#[derive(Parser, Debug)]
pub struct Opt {
    /// The name of the camera. Must be a name in the config
    pub camera: String,
}
