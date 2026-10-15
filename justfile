install:
	cargo install --path . --force
	goh install

dev:
	cargo run -- install -vvvv

git-show-hooks:
	git config --global --get core.hooksPath
