# 第三方依赖许可证说明

macOS 版本使用 pnpm 管理前端依赖、Cargo 管理 Rust 依赖。准确的版本和许可证以锁文件及各依赖发布包中的 LICENSE/NOTICE 为准，不在此文档复制第三方源码。

查看依赖清单：

```sh
cd macos
pnpm licenses list
cargo tree --manifest-path src-tauri/Cargo.toml
```

发布前应保存依赖清单并核对 MIT、Apache-2.0、BSD 等许可证的归属和 NOTICE 要求。若依赖许可证不兼容或缺少声明，应在合并前替换依赖或补充对应归属。项目本身暂不创建 `LICENSE`，等待维护者选择项目许可证。
