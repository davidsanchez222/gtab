# Regenerate this formula from the tagged release commit before copying it into
# your tap repository:
#   ./scripts/render-homebrew-formula.sh <release-commit-sha>
#
# The revision below reflects the last committed repo state at generation time.
class Gtab < Formula
  desc "Ghostty tab workspace manager with an interactive TUI"
  homepage "https://github.com/Franvy/gtab"
  url "https://github.com/Franvy/gtab.git",
      tag: "v1.3.0",
      revision: "75cfd3ed8d63af8eabffc12e7fea244a9289b5a0"
  version "1.3.0"
  license "MIT"
  head "https://github.com/Franvy/gtab.git", branch: "main"

  depends_on :macos
  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
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

    system bin/"gtab", "set", "close_tab", "on"
    assert_match "close_tab = on", shell_output("#{bin}/gtab set")
  end
end
