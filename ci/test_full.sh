#!/bin/bash

set -e

CRATE=gh-board
MSRV=1.88

get_rust_version() {
  local array=($(rustc --version));
  echo "${array[1]}";
  return 0;
}
RUST_VERSION=$(get_rust_version)

check_version() {
  IFS=. read -ra rust <<< "$RUST_VERSION"
  IFS=. read -ra want <<< "$1"
  [[ "${rust[0]}" -gt "${want[0]}" ||
   ( "${rust[0]}" -eq "${want[0]}" &&
     "${rust[1]}" -ge "${want[1]}" )
  ]]
}

echo "Testing $CRATE on rustc $RUST_VERSION"
if ! check_version $MSRV ; then
  echo "The minimum for $CRATE is rustc $MSRV"
  exit 1
fi

set -x

# Install cargo-nextest (pre-built binary)
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  NEXTEST_PLATFORM="x86_64-unknown-linux-gnu.tar.gz" ;;
  Linux-aarch64) NEXTEST_PLATFORM="aarch64-unknown-linux-gnu.tar.gz" ;;
  Darwin-*)      NEXTEST_PLATFORM="universal-apple-darwin.tar.gz" ;;
  *)             echo "Unsupported platform for cargo-nextest"; exit 1 ;;
esac
curl -sSfL "https://get.nexte.st/latest/${NEXTEST_PLATFORM}" | tar zx
chmod +x cargo-nextest
mv cargo-nextest ~/.cargo/bin/

# test the default build
cargo build
cargo nextest run
cargo test --doc
