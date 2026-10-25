# Relinking LAME (LGPL Compliance)

Streamy Desktop bundles `libmp3lame.dll` as a separate dynamically-linked shared library to comply with the GNU Lesser General Public License (LGPL v2.1+).

## Modifying and Replacing `libmp3lame.dll`

Users have the right to modify the LAME library and replace it with their own build:

1. **Obtain or edit the source code:**
   The exact vendored source of LAME is located in `crates/streamy-lame/vendor/lame`. You may modify any of the source files.

2. **Rebuild the DLL on Windows:**
   Open a PowerShell prompt with MSVC tools (or CMake installed) and run:
   ```powershell
   .\scripts\build-lame-dll.ps1
   ```
   This compiles `target\lame-dist\libmp3lame.dll`.

3. **Install the replacement:**
   Copy the newly generated `libmp3lame.dll` into the Streamy Desktop installation folder (next to `Streamy.exe`, e.g. `%LOCALAPPDATA%\Programs\Streamy Desktop\` or wherever installed), replacing the existing file.

4. **Launch Streamy Desktop:**
   The application will dynamically load your replaced `libmp3lame.dll`.
