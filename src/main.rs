use anyhow::{Result, bail};
use clap::Parser;
use gtab::{
    app,
    cli::{Cli, Commands},
    core::{AppEnv, format_settings, format_workspace_list},
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
        (Some(Commands::Init), None) => handle_init(&mut env),
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
        (Some(Commands::Rename { old, new }), None) => {
            if old == new {
                println!("Workspace name unchanged.");
                return Ok(());
            }

            env.rename_workspace(&old, &new)?;
            println!("Renamed workspace \"{old}\" to \"{new}\"");
            Ok(())
        }
        (Some(Commands::Remove { name }), None) => {
            env.remove_workspace(&name)?;
            println!("Removed workspace \"{name}\"");
            Ok(())
        }
        (Some(Commands::Set { key, value }), None) => {
            handle_set(&mut env, key.as_deref(), value.as_deref())
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
        (Some("launch_mode"), Some(_)) => {
            bail!("launch_mode has been removed; gtab only uses the Ghostty-local shortcut now")
        }
        (Some("global_shortcut"), Some(_)) => {
            bail!("global_shortcut has been removed; gtab only uses the Ghostty-local shortcut now")
        }
        (Some("ghostty_shortcut"), Some(shortcut)) => {
            let sync = env.set_ghostty_shortcut(shortcut)?;
            println!("Set ghostty_shortcut = {}", env.ghostty_shortcut_display());
            if env.ghostty_shortcut_display() == "off" {
                println!("Removed managed Ghostty shortcut reference from:");
                println!("  {}", sync.config_path.display());
                println!("Ghostty-local shortcut is now disabled.");
                println!(
                    "Reload Ghostty config or restart Ghostty to stop Cmd+G from typing `gtab`."
                );
            } else {
                println!(
                    "Managed Ghostty keybind file: {}",
                    sync.include_path.display()
                );
                println!("This shortcut types `gtab` into the focused Ghostty shell.");
                println!("Reload Ghostty config or restart Ghostty to apply the shortcut.");
            }
            Ok(())
        }
        (Some(_), _) => bail!("unknown setting"),
        _ => bail!("usage: gtab set <key> <value>"),
    }
}

fn handle_init(env: &mut AppEnv) -> Result<()> {
    let sync = env.init_shortcuts()?;
    println!("Initialized Ghostty-local shortcut.");
    println!("  ghostty_shortcut = {}", sync.shortcut);
    println!("  ghostty_config = {}", sync.config_path.display());
    println!("  ghostty_include = {}", sync.include_path.display());
    println!("Reload Ghostty config or restart Ghostty, then press Cmd+G inside Ghostty.");
    Ok(())
}
