# Where should we write the GIF?
Output ferrix.gif

# Set up a 1200x600 terminal with 46px font.
Set FontSize 20
Set Width 1280
Set Height 720

# Type a command in the terminal.
Type "cargo run --bin flemis_fs"

Sleep 500ms
Enter

Type "ls"
Sleep 500ms
Enter

Sleep 1000ms

Type "mkdir john"
Sleep 500ms
Enter

Type "ls"
Sleep 500ms
Enter

Sleep 2000ms

Type "cd john"
Sleep 500ms
Enter

Type "ls"
Sleep 500ms
Enter

Sleep 2000ms

Type "cd ../"
Sleep 500ms
Enter

Type "ls"
Sleep 500ms
Enter

Sleep 2000ms

Type "touch -n 10 lg"
Sleep 500ms
Enter

Type "ls"
Sleep 500ms
Enter

Type "head lg"
Sleep 500ms
Enter

Sleep 2000ms

Type "sort lg"
Sleep 500ms
Enter

Type "head lg"
Sleep 500ms
Enter

# Admire the output for a bit.
Sleep 5s
