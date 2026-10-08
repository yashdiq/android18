.PHONY: help apk install-apk dmg run test clean-dist

help:
	@echo "make apk          signed release APK -> dist/android18-<ver>.apk"
	@echo "make install-apk  install dist APK on the connected phone + launch"
	@echo "make dmg          macOS Android18.app + dist/Android18-<ver>.dmg"
	@echo "make run          dev loop (scripts/dev.sh)"
	@echo "make test         cargo tests + Android unit tests"

apk:
	scripts/build-apk.sh

install-apk:
	scripts/install-apk.sh

dmg:
	scripts/build-dmg.sh

run:
	scripts/dev.sh

test:
	cargo test --workspace
	cd android-service && ./gradlew --console=plain :app:testDebugUnitTest

clean-dist:
	rm -rf dist
