if ($args.Count -gt 0 -and $args[0] -eq "--version") {
  Write-Output "fixture-cli 0.1.0"
  exit 0
}

Write-Output "fixture CLI: no real user files are touched"
