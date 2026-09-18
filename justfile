install:
	cargo run -- install -vvvv

git-show-hooks:
	git config --global --get core.hooksPath
