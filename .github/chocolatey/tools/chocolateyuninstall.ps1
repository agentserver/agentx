$ErrorActionPreference = 'Stop'

# Chocolatey removes the package directory automatically, which also removes
# the shimmed agentx.exe from $env:ChocolateyInstall\bin. Nothing further to
# do here — agentx writes its config to %USERPROFILE%\.agentx but leaves that
# alone on uninstall (mirrors how unix package managers behave with dotfiles).
