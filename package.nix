{
  individualCrateArgs,
  fileSetForCrate,
  craneLib,
  lib,
  withCmd ? false,
}:
craneLib.buildPackage (
  individualCrateArgs
  // {
    pname = "kanata";
    cargoExtraArgs = "-p kanata" + (lib.optionalString withCmd " --features cmd");
    src = fileSetForCrate ./.;
  }
)
