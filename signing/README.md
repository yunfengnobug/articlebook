# 更新签名密钥

`articlebook.key` 是 Tauri updater 的 minisign 私钥，`articlebook.key.pub` 是对应公钥。

私钥是故意放进这个私有仓库的。密码为空，不要设置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。同一份私钥也放在 GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` 里，构建时作为备用。

丢失或换掉这把私钥之后，已经安装的客户端无法再验证新版本，自动更新会断掉。不要重新生成密钥，除非准备放弃现有安装的更新链。

这两个文件在 `.gitattributes` 里标成 `-text`，避免 Windows 上的 `core.autocrlf` 改写内容。构建时把 `TAURI_SIGNING_PRIVATE_KEY` 指到 `signing/articlebook.key` 即可，Tauri 接受密钥路径或密钥正文。
