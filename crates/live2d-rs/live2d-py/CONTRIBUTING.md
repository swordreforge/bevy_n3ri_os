[python c extension]: https://docs.python.org/3/c-api/index.html

[Core api 文档]: https://docs.live2d.com/en/cubism-sdk-manual/cubism-core-api-reference/

# 开发说明

[English](./CONTRIBUTING.en.md)

本项目涉及：CMake、Cubism Native SDK、Python C Extension、OpenGL。

* Cubism 相关部分可以查阅官方文档，这里推荐官方的 [Core api 文档]（可以下载 pdf），可以对整个 Live2D 绘制流程有一个整体把握。
* [Python c extension]
* 详细的 live2d.v2 渲染流程、算法和着色器实现请参考：[Live2D v2 渲染系统文档](./docs/LIVE2D_V2_RENDERER.md)

## 项目结构

整个项目由两个部分构成：live2d.v2 和 live2d.v3，分别对应 `package/live2d/v2` 和 `package/live2d/v3` 目录。

### live2d.v2

加载 Cubism 2.1 及以下的 live2d 模型。

live2d.v2 完全采用 Python 实现，通过工具对 live2d.min.js 反混淆、转 Python 生成，并辅以手动修复💦。在 live2d.min.js 的功能基础上，额外增加了点击部件的精确检测、部件颜色设置等功能。性能欠佳，~~因为保留了一部分 javascript 特性~~。

### live2d.v3

加载 Cubism 3.0 及以上的 live2d 模型。

live2d.v3 使用 [python c extension] 对 Cubism Native SDK 进行封装。

## live2d.v3 构成

live2d.v3 使用 Python 可以调用的动态库 (`.pyd` 或 `.so`)，由 CMake 管理的项目编译生成。

live2d.v3 的 C++ 模块包括：Core、Framework、Main、Wrapper 四个模块。

### Core

Cubism Native Core，包括一个头文件`.h`和若干平台对应的静态库。用于读取 Cubism 3.0 及以上 live2d 模型的 `.moc3` 文件。

### Framework
Cubism Native Framework，在 Core 层上的拓展，比如 json 文件读取、物理计算、图形绘制等。

> 上面两个模块由 Cubism 官方发布。在官方发布新版本后可以直接替换，几乎不需要做修改（目前由 CMake 自动化修改）。

### Main
对应原来 Cubism Native SDK 的应用层，对其进行了精简。Main 在 Framework 基础上实现了一个可以绘制的 `LAppModel` cpp 类，增改功能主要是修改 `Live2D/Main/src/LAppModel.cpp` 中定义的类。Main 中的其他文件几乎很少改动。

Framework 和 Main 会分别生成自己的静态库。

> 上面的三个模块和 Python 无关，也可以用于绑定其他编程语言。

### Wrapper
将 Main 中实现的 `LAppModel` cpp 类封装为 Python 模块，是整个项目中唯一引入 Python 相关依赖的位置。

Wrapper 模块位于 `Wrapper/` 目录，包含以下文件：
- `Live2D.cpp` - 主封装文件
- `PyLAppModel.cpp` / `PyLAppModel.hpp` - LAppModel 的 Python 绑定
- `PyModel.cpp` / `PyModel.hpp` - Model 的 Python 绑定
- `Python.hpp` - Python API 头文件

### 编译过程

项目使用 CMake 构建系统，并通过 `setup.py` 进行集成：

1. **SDK 下载**：`setup.py` 会自动从 Live2D 官方下载 Cubism Native SDK
2. **CMake 构建**：调用 CMake 编译各模块
   - Framework 模块编译生成静态库
   - Main 模块编译生成静态库
   - Wrapper 模块编译生成 `Live2DWrapper` 动态库
3. **文件复制**：构建完成后，动态库和着色器文件会被复制到 `package/live2d/v3/` 目录

### 构建命令

```bash
# 从源码构建
pip install .

# 仅下载 SDK
python setup.py download

# 构建 wheel 包
python setup.py bdist_wheel
```

详细构建说明请参考 [Wiki - 源码构建](https://github.com/Arkueid/live2d-py/wiki/%E5%AE%89%E8%A3%85#%E6%BA%90%E7%A0%81%E6%9E%84%E5%BB%BA)

## 平台支持

当前已支持以下平台的构建：

- Windows (x86/x64)
- Linux (x64/ARM64)
- macOS (Intel/Apple Silicon, Python >=3.11)

各平台的构建 workflow 已在 GitHub Actions 中配置完成。

## 待完成
* live2d.v2 的性能提升
