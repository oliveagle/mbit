PREFIX ?= $(HOME)/.local/bin

.PHONY: install
install:
	cargo build --release
	@mkdir -p $(PREFIX)
	install -m 755 target/release/mbit $(PREFIX)/mbit
	@echo "Installed mbit to $(PREFIX)/mbit"
