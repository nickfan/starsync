#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?usage: generate-homebrew-formula.sh VERSION URL SHA256 [OUTPUT]}"
URL="${2:?usage: generate-homebrew-formula.sh VERSION URL SHA256 [OUTPUT]}"
SHA256="${3:?usage: generate-homebrew-formula.sh VERSION URL SHA256 [OUTPUT]}"
OUTPUT="${4:-}"

VERSION="${VERSION#v}"

render_formula() {
  cat <<RUBY
class Starsync < Formula
  desc "Local-first GitHub starred repository knowledge sync"
  homepage "https://github.com/nickfan/starsync"
  url "${URL}"
  sha256 "${SHA256}"
  license "Apache-2.0"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--offline", "--path", ".", "--root", prefix
  end

  service do
    run [opt_bin/"starsync", "serve"]
    keep_alive true
    working_dir HOMEBREW_PREFIX
    log_path var/"log/starsync.log"
    error_log_path var/"log/starsync.err.log"
    environment_variables PATH: std_service_path_env
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/starsync --version")
    assert_match "Local-first", shell_output("#{bin}/starsync --help")
  end
end
RUBY
}

if [[ -n "${OUTPUT}" ]]; then
  mkdir -p "$(dirname "${OUTPUT}")"
  render_formula > "${OUTPUT}"
else
  render_formula
fi
