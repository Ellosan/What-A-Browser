//! The `wat` binary: opens the browser window, or runs a headless command.

use std::process::ExitCode;

use wat_cli::{parse_args, run, Command, USAGE};
use wat_shell::ShellConfig;

fn main() -> ExitCode {
    // `RUST_LOG=debug wat …` turns on engine logging.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match parse_args(&args) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("wat: {error}\n");
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    if let Command::Gui { url } = &invocation.command {
        let options = &invocation.options;
        let theme = wat_theme::Theme::named(&options.theme);
        if let Err(error) = &theme {
            eprintln!("wat: {error}");
            return ExitCode::FAILURE;
        }
        let config = ShellConfig {
            url: url.clone(),
            size: options.size(),
            theme: options.theme.clone(),
            appearance: options.appearance.map(|dark| {
                if dark {
                    wat_theme::Appearance::Dark
                } else {
                    wat_theme::Appearance::Light
                }
            }),
            offline: options.offline,
            touch: options.mobile,
            search: options.search.clone(),
            home: "about:home".to_string(),
        };
        return match wat_shell::run(config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("wat: {error}");
                eprintln!(
                    "\nNo window could be opened. The headless commands still work:\n\
                     \n    wat shot <URL> -o shot.png\n    wat render <URL> -o page.png\n\
                     \nRun `wat help` for the full list."
                );
                ExitCode::FAILURE
            }
        };
    }

    match run(&invocation) {
        Ok(output) => {
            print!("{output}");
            if !output.ends_with('\n') {
                println!();
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("wat: {error}");
            ExitCode::FAILURE
        }
    }
}
