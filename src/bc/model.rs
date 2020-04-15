pub enum Bc {
    LegacyMsg(LegacyMsg),
    ModernMsg(ModernMsg),
    Unknown,
}

pub struct ModernMsg {

}

pub enum LegacyMsg {}
