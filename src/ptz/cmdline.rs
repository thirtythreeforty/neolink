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
    /// Move to a stored preset
    Preset { preset_id: Option<u8> },
    /// Assign the current position to a preset with a given name
    Assign { preset_id: u8, name: String },
    /// Performs a movement in the given direction
    Control {
        /// The amount to move
        amount: u32,
        /// The direction command
        #[clap(value_enum)]
        command: CmdDirection,
        /// The speed to move at
        speed: Option<u32>,
    },
}
