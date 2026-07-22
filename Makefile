PREFIX ?= $(HOME)/.local
BINDIR = $(PREFIX)/bin

.PHONY: build release install uninstall check test clean

build:
	cargo build

release:
	cargo build --release

install: release
	install -Dm755 target/release/dothoard $(BINDIR)/dothoard
	@echo ""
	@echo "Installed dothoard to $(BINDIR)/dothoard"
	@echo ""
	@case ":$$PATH:" in \
		*":$(BINDIR):"*) ;; \
		*) echo "NOTE: $(BINDIR) is not in your PATH."; \
		   echo "Add it with:  fish_add_path $(BINDIR)  (fish)"; \
		   echo "         or:  export PATH=\"$(BINDIR):\$$PATH\"  (bash/zsh)"; \
		   echo "" ;; \
	esac

uninstall:
	rm -f $(BINDIR)/dothoard

check:
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-targets --all-features -- --test-threads=1

clean:
	cargo clean
