{
  lib,
  rustPlatform,
  installShellFiles,
  stdenv,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
  canExecute = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  version = cargoToml.package.version;

  src = lib.cleanSource ../.;
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [ installShellFiles ];
  doCheck = canExecute;

  postInstall = lib.optionalString canExecute ''
    installShellCompletion --cmd reddit \
      --bash <("$out/bin/reddit" completions bash) \
      --zsh <("$out/bin/reddit" completions zsh) \
      --fish <("$out/bin/reddit" completions fish)
  '';

  meta = {
    description = cargoToml.package.description;
    homepage = "https://github.com/Microck/reddit-cli";
    license = lib.licenses.mit;
    mainProgram = "reddit";
    platforms = lib.platforms.unix;
  };
}
