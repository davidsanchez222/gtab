use anyhow::{Result, bail};
use clap::Parser;
use gtab::{
    app,
    cli::{Cli, Commands, HotkeyCommands},
    core::{
        AppEnv, format_hotkey_doctor, format_hotkey_status, format_settings, format_shortcut_guide,
        format_workspace_list,
    },
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.version {
        println!("gtab {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut env = AppEnv::load()?;

    match (cli.command, cli.workspace) {
        (None, None) => app::run_tui(&mut env),
        (None, Some(name)) => {
            println!("Launching \"{name}\"...");
            env.launch_workspace(&name)
        }
        (Some(Commands::Tui), None) => app::run_tui(&mut env),
        (Some(Commands::List), None) => {
            let workspaces = env.list_workspaces()?;
            println!("{}", format_workspace_list(&workspaces));
            Ok(())
        }
        (Some(Commands::Save { name }), None) => {
            let path = env.save_current_window(&name)?;
            println!("Saved workspace \"{name}\"");
            println!("  {}", path.display());
            Ok(())
        }
        (Some(Commands::Edit { name }), None) => env.open_in_editor(&name),
        (Some(Commands::Remove { name }), None) => {
            env.remove_workspace(&name)?;
            println!("Removed workspace \"{name}\"");
            Ok(())
        }
        (Some(Commands::Set { key, value }), None) => {
            handle_set(&mut env, key.as_deref(), value.as_deref())
        }
        (Some(Commands::Hotkey { command }), None) => handle_hotkey(&env, command),
        (Some(Commands::Shortcut), None) => {
            let launcher_path = env.ensure_launcher_script()?;
            println!("{}", format_shortcut_guide(&env, &launcher_path));
            Ok(())
        }
        _ => bail!("unexpected CLI arguments"),
    }
}

fn handle_set(env: &mut AppEnv, key: Option<&str>, value: Option<&str>) -> Result<()> {
    match (key, value) {
        (None, None) => {
            println!("{}", format_settings(env));
            Ok(())
        }
        (Some("close_tab"), Some("on" | "true")) => {
            env.set_close_tab(true)?;
            println!("Set close_tab = on");
            Ok(())
        }
        (Some("close_tab"), Some("off" | "false")) => {
            env.set_close_tab(false)?;
            println!("Set close_tab = off");
            Ok(())
        }
        (Some("close_tab"), Some(_)) => bail!("close_tab value must be 'on' or 'off'"),
        (Some("global_shortcut"), Some(shortcut)) => {
            env.set_global_shortcut(shortcut)?;
            println!("Set global_shortcut = {}", env.global_shortcut_display());
            match env.restart_hotkey_agent() {
                Ok(status) => println!(
                    "{}",
                    format_hotkey_status(&status, env.ghostty_shortcut_display())
                ),
                Err(error) => {
                    println!("Hotkey helper restart failed: {error}");
                    println!("Run `gtab hotkey install` after installing both binaries.");
                }
            }
            Ok(())
        }
        (Some("ghostty_shortcut"), Some(shortcut)) => {
            let sync = env.set_ghostty_shortcut(shortcut)?;
            println!("Set ghostty_shortcut = {}", env.ghostty_shortcut_display());
            println!(
                "Managed Ghostty keybind file: {}",
                sync.include_path.display()
            );
            if env.ghostty_shortcut_display() == "off" {
                println!("Legacy Ghostty text-injection shortcut is now disabled.");
                println!("Use the built-in hotkey helper with `gtab hotkey doctor` if needed.");
            } else {
                println!("This legacy shortcut types `gtab` into the focused shell.");
                println!(
                    "For Claude Code / Codex, rely on the built-in global hotkey helper instead."
                );
                println!("Reload Ghostty config or restart Ghostty to apply the shortcut.");
            }
            Ok(())
        }
        (Some(_), _) => bail!("unknown setting"),
        _ => bail!("usage: gtab set <key> <value>"),
    }
}

fn handle_hotkey(env: &AppEnv, command: Option<HotkeyCommands>) -> Result<()> {
    match command.unwrap_or(HotkeyCommands::Status) {
        HotkeyCommands::Install => {
            let status = env.install_hotkey_agent()?;
            println!(
                "{}",
                format_hotkey_status(&status, env.ghostty_shortcut_display())
            );
            Ok(())
        }
        HotkeyCommands::Restart => {
            let status = env.restart_hotkey_agent()?;
            println!(
                "{}",
                format_hotkey_status(&status, env.ghostty_shortcut_display())
            );
            Ok(())
        }
        HotkeyCommands::Status => {
            let status = env.hotkey_agent_status()?;
            println!(
                "{}",
                format_hotkey_status(&status, env.ghostty_shortcut_display())
            );
            Ok(())
        }
        HotkeyCommands::Doctor => {
            let status = env.hotkey_agent_status()?;
            println!(
                "{}",
                format_hotkey_doctor(
                    &status,
                    env.ghostty_shortcut_display(),
                    &env.hotkey_log_path()
                )
            );
            Ok(())
        }
        HotkeyCommands::Uninstall => {
            env.uninstall_hotkey_agent()?;
            println!("Hotkey helper uninstalled.");
            Ok(())
        }
    }
}
