def main [] {
  let item = ["crates", "docs",
    "PLAN.md","CHANGELOG.md",
    "Cargo.toml","README.md","rustfmt.toml"]
    ouch compress  -y ...$item ("compnode-revamped-" + (date now | format date "%Y-%m-%d %H-%M") + ".zip")

}
