#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod activity;
mod app;
mod attach;
mod auto;
mod awake;
mod browser;
mod cli;
mod composer;
mod history;
mod icons;
mod inputs;
mod install;
mod markdown;
mod models;
mod panes;
#[cfg(debug_assertions)]
mod preview;
mod providers;
mod runner;
mod speak;
mod speech;
mod storage;
mod terminal;
mod theme;
mod tutorial;
mod usage_view;
mod views;
mod working;

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let options = match cli::Options::parse(env::args_os().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            cli::attach_console();
            eprintln!("{message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    if options.is_terminal_command() {
        cli::attach_console();
        return cli::execute(options);
    }
    match app::launch(options.project, options.preview) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
