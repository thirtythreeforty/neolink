use structopt::StructOpt;

/// The pir command will read the PIR status of the camera
#[derive(StructOpt, Debug)]
pub struct Opt {
    /// The name of the camera. Must be a name in the config
    pub camera: String,
}
