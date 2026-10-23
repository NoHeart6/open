@echo off
cd /d "%~dp0"
py -3 bridge.py install
if errorlevel 1 pause & exit /b 1
py -3 bridge.py start
if errorlevel 1 pause & exit /b 1
echo Restart Roblox Studio, then open Plugins > ChatGPT Bridge.
pause
