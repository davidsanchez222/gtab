use anyhow::{Context, Result, bail};
use clap::Parser;
use gtab::{
    app,
    cli::{Cli, Commands},
    core::{
        AppEnv, GhosttyShortcutApplyResult, GhosttyShortcutApplyStatus, format_settings,
        format_workspace_list,
    },
};
use std::path::Path;

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

    let shell_cd = cli.shell_cd;
    let mut env = AppEnv::load()?;

    match (cli.command, cli.workspace) {
        (None, None) => {
            let exit = app::run_tui(&mut env)?;
            handle_tui_exit(&env, exit, shell_cd)
        }
        (None, Some(name)) => {
            println!("Launching \"{name}\"...");
            env.launch_workspace(&name)
        }
        (Some(Commands::Tui), None) => {
            let exit = app::run_tui(&mut env)?;
            handle_tui_exit(&env, exit, shell_cd)
        }
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

fn handle_tui_exit(env: &AppEnv, exit: app::TuiExit, shell_cd: bool) -> Result<()> {
    match exit {
        app::TuiExit::None => Ok(()),
        app::TuiExit::Cd(path) if shell_cd => {
            println!("{}", render_shell_cd_command(&path));
            Ok(())
        }
        app::TuiExit::Cd(path) => env
            .open_directory_in_focused_terminal(&path)
            .with_context(|| {
                "directory open failed. You can also run `gtab --shell-cd` through a shell wrapper fallback."
            }),
        app::TuiExit::ReplaceSplit(path) if shell_cd => {
            println!("{}", render_shell_cd_command(&path));
            Ok(())
        }
        app::TuiExit::ReplaceSplit(path) => env
            .replace_directory_in_focused_terminal(&path)
            .with_context(|| {
                "directory replace failed. This action swaps the current split with a fresh shell in the target directory."
            }),
    }
}

fn render_shell_cd_command(path: &Path) -> String {
    let path = path.to_string_lossy();
    format!("cd -- '{}'", shell_single_quote_escape(&path))
}

fn shell_single_quote_escape(value: &str) -> String {
    value.replace('\'', r#"'"'"'"#)
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
            let result = env.set_ghostty_shortcut(shortcut)?;
            println!("Set ghostty_shortcut = {}", env.ghostty_shortcut_display());
            if env.ghostty_shortcut_display() == "off" {
                print_disabled_shortcut_result(&result);
            } else {
                print_enabled_shortcut_result(&result);
            }
            Ok(())
        }
        (Some(_), _) => bail!("unknown setting"),
        _ => bail!("usage: gtab set <key> <value>"),
    }
}

fn handle_init(env: &mut AppEnv) -> Result<()> {
    let result = env.init_shortcuts()?;
    println!("Initialized Ghostty-local shortcut.");
    println!("  ghostty_shortcut = {}", result.sync.shortcut);
    println!("  ghostty_config = {}", result.sync.config_path.display());
    println!("  ghostty_include = {}", result.sync.include_path.display());

    if result.status == GhosttyShortcutApplyStatus::ManualConfigRequired {
        print_manual_config_addition(&result);
    } else {
        println!("Reload Ghostty config or restart Ghostty, then press Cmd+G inside Ghostty.");
    }

    Ok(())
}

fn print_enabled_shortcut_result(result: &GhosttyShortcutApplyResult) {
    println!(
        "Managed Ghostty keybind file: {}",
        result.sync.include_path.display()
    );
    println!("This shortcut types `gtab` into the focused Ghostty shell.");

    if result.status == GhosttyShortcutApplyStatus::ManualConfigRequired {
        print_manual_config_addition(result);
    } else {
        println!("Reload Ghostty config or restart Ghostty to apply the shortcut.");
    }
}

fn print_disabled_shortcut_result(result: &GhosttyShortcutApplyResult) {
    if result.status == GhosttyShortcutApplyStatus::ManualConfigRemovalRequired {
        println!(
            "Removed the local managed Ghostty shortcut file: {}",
            result.sync.include_path.display()
        );
        print_manual_config_removal(result);
    } else {
        println!("Removed managed Ghostty shortcut reference from:");
        println!("  {}", result.sync.config_path.display());
        println!("Ghostty-local shortcut is now disabled.");
        println!("Reload Ghostty config or restart Ghostty to stop Cmd+G from typing `gtab`.");
    }
}

fn print_manual_config_addition(result: &GhosttyShortcutApplyResult) {
    if let Some(reason) = &result.reason {
        println!("{reason}");
    }
    println!("Add this line to your Ghostty config source (for example Nix/Home Manager):");
    println!("  {}", result.include_config_line());
    println!("Rebuild/apply your config, then reload or restart Ghostty.");
}

fn print_manual_config_removal(result: &GhosttyShortcutApplyResult) {
    if let Some(reason) = &result.reason {
        println!("{reason}");
    }
    println!("Remove this line from your Ghostty config source (for example Nix/Home Manager):");
    println!("  {}", result.include_config_line());
    println!("Rebuild/apply your config, then reload or restart Ghostty to disable Cmd+G.");
}

#[cfg(test)]
mod tests {
    use super::render_shell_cd_command;
    use std::path::Path;

    #[test]
    fn render_shell_cd_command_wraps_path_for_shell_eval() {
        assert_eq!(
            render_shell_cd_command(Path::new("/tmp/demo")),
            "cd -- '/tmp/demo'"
        );
    }

    #[test]
    fn render_shell_cd_command_escapes_single_quotes() {
        assert_eq!(
            render_shell_cd_command(Path::new("/tmp/it'works")),
            "cd -- '/tmp/it'\"'\"'works'"
        );
    }
}
