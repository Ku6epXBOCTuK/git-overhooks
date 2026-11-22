cargo_bin := env_var("CARGO_HOME") + "/bin/goh.exe"

install:
	cargo install --path . --force
	"{{cargo_bin}}" install

dev:
	cargo run -- install -vvvv

git-show-hooks:
	git config --global --get core.hooksPath
