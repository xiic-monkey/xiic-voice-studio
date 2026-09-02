# macOS 发布

普通 push 和 Pull Request 只运行 `.github/workflows/ci.yml`。推送 `v*` tag 或手动运行 `.github/workflows/release.yml` 才会构建 macOS 发布包。

GitHub 仓库需要配置以下 Actions secrets：

- `APPLE_CERTIFICATE`：Developer ID Application `.p12` 证书的 Base64 内容。
- `APPLE_CERTIFICATE_PASSWORD`：`.p12` 导出密码。
- `APPLE_SIGNING_IDENTITY`：Developer ID Application 证书名称。
- `APPLE_ID`：Apple Developer 账号邮箱。
- `APPLE_PASSWORD`：该账号的 app-specific password。
- `APPLE_TEAM_ID`：Apple Developer Team ID。

未配置这些 secrets 时，不应把 release workflow 产物作为正式分发包。正式发布前还要在 Apple Developer 后台确认公证记录，并在干净 macOS 用户环境安装验证。
