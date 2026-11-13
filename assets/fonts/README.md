# Fonts

`MaterialSymbolsRounded-subset.ttf` is a subset of Google's
[Material Symbols](https://fonts.google.com/icons) (Rounded, FILL 1,
weight 400, optical size 24), licensed under Apache License 2.0.

It only contains the icons used by the app. To add one, append its name to
`icon_names` (alphabetical order), re-download, and add the codepoint to
`src/ui/icons.rs`:

```sh
NAMES="add,brightness_6,cancel,check_circle,close,content_cut,contrast,delete,error,fast_forward,fast_rewind,filter_b_and_w,folder_open,fullscreen,fullscreen_exit,movie_edit,pause,play_arrow,restart_alt,skip_next,skip_previous,transform,video_library"
URL=$(curl -s "https://fonts.googleapis.com/css2?family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@24,400,1,0&icon_names=$NAMES" | grep -o 'https://[^)]*')
curl -s "$URL" -o MaterialSymbolsRounded-subset.ttf
```
