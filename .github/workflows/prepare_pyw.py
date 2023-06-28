import json
import subprocess
import tomlkit

cargo_locate = subprocess.check_output(["cargo", "locate-project", "--workspace"])
cargo_path = json.loads(cargo_locate)["root"]
bin_section = [{"name": "pyw", "path": "src/main.rs", "required-features": ["pythonw"]}]

with open(cargo_path, "r", encoding="utf-8") as f:
    cargo_toml = tomlkit.load(f)

cargo_toml["bin"] = bin_section

with open(cargo_path, "w", encoding="utf-8") as f:
    tomlkit.dump(cargo_toml, f)

print("OK")
