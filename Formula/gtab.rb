class Gtab < Formula
  desc "Ghostty tab workspace manager with an interactive TUI"
  homepage "https://github.com/Franvy/gtab"
  url "https://github.com/Franvy/gtab.git",
      tag: "v1.3.1",
      revision: "650e7d3747b85d783f2a0211b255d5e38395c3ed"
  version "1.3.1"
  license "MIT"
  head "https://github.com/Franvy/gtab.git", branch: "main"

  depends_on :macos
  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  def caveats
    <<~EOS
      Run this once to enable the built-in global hotkey helper:
        gtab hotkey install

      Workspaces are stored in ~/.config/gtab/ by default.
      Override with: export GTAB_DIR="/your/path"

      Requires Ghostty terminal: https://ghostty.org
    EOS
  end

  test do
    ENV["GTAB_DIR"] = testpath/"gtab"
    (testpath/"gtab").mkpath
    (testpath/"gtab/demo.applescript").write <<~APPLESCRIPT
      tell application "Ghostty"
      end tell
    APPLESCRIPT

    assert_match version.to_s, shell_output("#{bin}/gtab --version")
    assert_match "demo", shell_output("#{bin}/gtab list")
    assert_match "close_tab = off", shell_output("#{bin}/gtab set")
    assert_match "launch_mode = smart", shell_output("#{bin}/gtab set")
    assert_match "global_shortcut = cmd+g", shell_output("#{bin}/gtab set")

    system bin/"gtab", "set", "close_tab", "on"
    assert_match "close_tab = on", shell_output("#{bin}/gtab set")

    system bin/"gtab", "set", "launch_mode", "window"
    assert_match "launch_mode = window", shell_output("#{bin}/gtab set")
  end
end
