fn main() {
	println!("cargo:rustc-env=VERGEN_GIT_SHA=0c11678d72a636c3a4bc723db87e03e7b888eaf9");
	println!("cargo:rustc-env=VERGEN_GIT_DIRTY=false");
	println!("cargo:rustc-env=VERGEN_RUSTC_SEMVER=wasix");
	println!("cargo:rustc-env=VERGEN_RUSTC_HOST_TRIPLE=wasm32-wasmer-wasi");
}
