# Session Log: 2026-08-24

## 本次工作内容

### 1. 窗口边界约束
- 窗口被约束在顶部栏(32px)和底部Dock(66px)之间
- 初始位置: `top: 72px`
- 拖拽范围: `top: [32, screen_height - 66]`
- 最大化: 填满中间可用区域

### 2. Dock Magnification 冻结
- 新增 `IsDragging` 资源
- 拖拽窗口时冻结 dock magnification
- 拖拽结束后恢复正常

### 3. 窗口吸附(Snap-to-Edge)
- 新增 `snap.rs` 模块
- 热区域: 左/右半屏(20px边缘), 最大化(顶部20px), 四角(80px)
- 蓝色半透明预览框
- 松手自动放置

### 4. 窗口边缘调整(Resize)
- 新增 `resize.rs` 模块
- 8个调整区域: 4边 + 4角(8px手柄)
- 光标图标变化
- 最小尺寸约束: 200x100

### 5. macOS风格圆角
- 窗口圆角: `border_radius: 10px` + `overflow: hidden()`
- 标题栏圆角: 顶部左右10px
- 三色按钮: `border_radius: 6px` (正圆)
- 标题居中: 三段式布局(按钮 | 标题 | 占位)

### 6. Dock功能完善
- 悬停提示(Tooltip)
- 应用启动/隐藏切换
- 蓝色运行指示器(Running Indicator)
- 关闭后可重新打开

---

## 教训总结

### 教训1: B0001 - 查询冲突

**问题**: Bevy ECS 中 `Query<&mut T>` 和 `Query<&T>` 不能同时存在于两个系统中。

**错误示例**:
```rust
// 系统A: 需要 &mut Visibility
fn system_a(mut query: Query<&mut Visibility>) { ... }

// 系统B: 需要 &Visibility (冲突!)
fn system_b(query: Query<&Visibility>) { ... }
```

**解决方案**:

1. **合并系统**: 把冲突的系统合并成一个
2. **使用 Commands**: 不直接查询，而是通过 `commands.entity().insert()` 延迟修改
3. **使用标记组件**: 创建自定义标记组件代替直接查询原生组件

**本次实战**:
```rust
// 错误: dock_update 查询 &mut Visibility on AppWindow
fn dock_update(
    mut window_query: Query<(Entity, &AppWindow, &mut Visibility)>,
    mut indicator_query: Query<&mut Visibility, With<RunningIndicator>>,
) { ... }

// 正确: 使用 AppVisible 标记 + Commands
fn dock_update(
    mut window_query: Query<(Entity, &AppWindow, &mut AppVisible)>,
    mut indicator_query: Query<&mut Visibility, With<RunningIndicator>>,
    mut commands: Commands,
) {
    // 状态变化通过 commands.entity().insert() 延迟应用
    commands.entity(entity).insert(Visibility::Hidden);
}
```

### 教训2: Bevy 0.19 API 变化

| 旧API | 新API | 说明 |
|-------|-------|------|
| `despawn_recursive()` | `despawn()` | 直接删除即可 |
| `Parent` | `ChildOf` | 需要 `use Relationship` trait |
| `ZIndex::Global(-1)` | `GlobalZIndex(-1)` | 分离了 |
| `BorderColor(Color)` | `BorderColor::all(color)` | 改为方法调用 |
| `Flex` enum | `flex_grow`/`flex_shrink` | 不再有Flex枚举 |
| `Window.cursor.icon` | `CursorIcon` 组件 | 需要 `commands.entity().insert()` |

### 教训3: 查询架构设计

**原则**: 
- 同一组件不要被多个系统以不同访问模式查询
- 使用标记组件(Component Marker)隔离状态查询
- 延迟修改用 `Commands`，立即读取用 `Query`

**架构模式**:
```
AppVisible(bool)  ← 标记状态
    ↓
Commands           ← 应用实际 Visibility 变化
    ↓
Visibility         ← Bevy 渲染使用
```

### 教训4: 热区域设计

**边缘热区域要遵守窗口边界**:
- 屏幕边缘 ≠ 可用区域边缘
- 顶部栏(32px)和底部Dock(66px)不计入可用区域
- 热区域应该在可用区域的边缘/角落

### 教训5: 光标图标系统

**Bevy 0.19 中 CursorIcon 是组件**:
```rust
// 不是设置 Window.cursor.icon
// 而是 insert CursorIcon 组件
commands.entity(window_entity).insert(CursorIcon::System(SystemCursorIcon::EwResize));

// 移除时触发默认光标恢复
commands.entity(window_entity).remove::<CursorIcon>();
```

---

## 文件变更总结

| 文件 | 变更 |
|------|------|
| `crates/n3ri-ui/src/snap.rs` | 新建: 窗口吸附功能 |
| `crates/n3ri-ui/src/resize.rs` | 新建: 窗口边缘调整 |
| `crates/n3ri-ui/src/dock.rs` | 重构: AppVisible, B0001修复 |
| `crates/n3ri-ui/src/window.rs` | 增强: 圆角, 边界约束, AppVisible |
| `crates/n3ri-ui/src/lib.rs` | 注册新插件 |
| `examples/minimal/src/main.rs` | 未修改 |

## Git 提交记录

```
869b8f2 fix: use Commands for visibility toggle to avoid B0001
1bb200e fix: use AppVisible marker to avoid B0001 Visibility conflict
2ff61a1 fix: merge dock systems to resolve B0001 query conflict
682c4b7 feat: blue running indicator dot below dock icons
139ec8c fix: reopen app windows from dock after close
702eb54 style: centered title text and rounded title bar corners
b3ef546 style: add macOS-style rounded corners to windows and traffic lights
a344c00 feat: dock app tooltips on hover
2c78f8a feat: window resize-by-edge with cursor icons
14c4749 feat: window snap-to-edge with blue preview overlay
a8c04fe fix: snap hot zones respect usable area boundaries
f60a972 fix: dock magnification only triggers when cursor near dock vertically
19ccf81 fix: freeze dock magnification during window drag
d86b9d2 feat: dock app launch with show/hide toggle
```
