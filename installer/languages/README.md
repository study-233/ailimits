# Installer translations

`ChineseSimplified.isl` is vendored from
[kira-96/Inno-Setup-Chinese-Simplified-Translation](https://github.com/kira-96/Inno-Setup-Chinese-Simplified-Translation)
at commit `1ff90acc4ed4aee82b1cda43253243deee3daed4` (Inno Setup 6.5.0+).
Its MIT license is retained in `ChineseSimplified.LICENSE.txt` and included
in the installed application's `licenses` directory.

The installer uses the repository copy so local builds and CI have the same
translation without depending on optional compiler language downloads.
English uses Inno Setup's `Default.isl`. Only English and Simplified Chinese
are offered by the installer; these choices do not change the app's language
preference.
