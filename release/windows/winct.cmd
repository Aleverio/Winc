@rem Starts winct from cmd or PowerShell. --launcher lets winct.exe start its window in the background,
@rem so the command returns instead of waiting for winct to exit.
@"%~dp0..\winct.exe" --launcher %*
