Type "head -22 desk.toml"
Enter
Sleep 2500ms
Type "manfred check desk.toml && echo valid"
Enter
Wait@10s /^\$\s*$/
Sleep 1s
Type "manfred build desk.toml --out out"
Enter
Wait@10s /^\$\s*$/
Sleep 1s
Type "manfred build desk.toml --out again && sha256sum out/desk.glb again/desk.glb"
Enter
Wait@10s /^\$\s*$/
Sleep 3s
