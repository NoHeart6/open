@echo off
cd /d "%~dp0"
py -3 bridge.py start
if errorlevel 1 pause
