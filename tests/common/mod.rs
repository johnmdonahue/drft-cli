use std::process::Command;

pub fn drft_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_drft"))
}
