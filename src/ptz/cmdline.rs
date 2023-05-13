use clap::Parser;

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum CmdDirection {
    Left,
    Right,
    Up,
    Down,
    In,
    Out,
    Stop,
}

/// The ptz command will control the positioning of the camera
#[derive(Parser, Debug)]
pub struct Opt {
    /// The name of the camera to change the lights of. Must be a name in the config
    pub camera: String,

    #[command(subcommand)]
    pub cmd: PtzCommand,
}

#[derive(Parser, Debug)]
pub enum PtzCommand {
    /// Gets the available presets on the camera, moves the camera to a given preset ID or saves
    /// the current position as a preset with name and ID.
    Preset {
        preset_id: Option<i8>,
        name: Option<String>,
    },
    /// Performs a movement in the given direction
    Control {
        /// The time in milliseconds to move
        duration: u32,
        /// The direction command
        #[clap(value_enum)]
        command: CmdDirection,
    },
}
