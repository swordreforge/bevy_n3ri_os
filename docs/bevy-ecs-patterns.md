# Bevy ECS 踩坑指南

## B0001 错误详解

### 什么是 B0001?

```
error[B0001]: system X accesses component(s) Y in a way that conflicts 
with a previous system parameter
```

Bevy 的 ECS 架构在**编译期**检查系统间的组件访问冲突。如果两个系统可能同时访问同一个组件（一个读，一个写），Bevy 会拒绝编译。

### 冲突的两种情况

#### 1. 跨系统冲突（最常见）

```rust
// 系统A 需要修改
fn system_a(mut q: Query<&mut Visibility>) { ... }

// 系统B 需要读取（冲突！）
fn system_b(q: Query<&Visibility>) { ... }
```

#### 2. 同系统内的查询冲突

```rust
fn my_system(
    mut q1: Query<&mut Visibility, With<AppWindow>>,
    mut q2: Query<&mut Visibility, With<DockTooltip>>,  // 可能冲突！
) { ... }
```

### 解决方案

#### 方案1: 合并系统（推荐）

```rust
// 把冲突的系统合并成一个
fn combined_system(
    mut q1: Query<(Entity, &AppWindow, &mut AppVisible)>,
    mut indicator_query: Query<&mut Visibility, With<RunningIndicator>>,
) {
    // 在同一个系统内安全访问
}
```

#### 方案2: 使用 Commands 延迟修改

```rust
fn system_a(
    q: Query<(Entity, &AppWindow, &mut AppVisible)>,
    mut commands: Commands,
) {
    // 不直接修改 Visibility
    // 而是通过 Commands 延迟应用
    commands.entity(entity).insert(Visibility::Hidden);
}
```

#### 方案3: 使用标记组件（本次实战）

```rust
// 定义标记组件
#[derive(Component)]
struct AppVisible(pub bool);

// 系统A: 只查询标记
fn system_a(mut q: Query<&mut AppVisible>) { ... }

// 系统B: 通过 Commands 设置实际组件
fn system_b(
    q: Query<(&AppVisible, Entity)>,
    mut commands: Commands,
) {
    for (vis, entity) in q.iter() {
        if vis.0 {
            commands.entity(entity).insert(Visibility::Inherited);
        } else {
            commands.entity(entity).insert(Visibility::Hidden);
        }
    }
}
```

#### 方案4: 使用 Without 过滤器

```rust
// 证明两个查询不重叠
fn system(
    mut q1: Query<&mut Visibility, With<AppWindow>>,
    mut q2: Query<&mut Visibility, (With<DockTooltip>, Without<AppWindow>)>,
) { ... }
```

#### 方案5: 使用 ParamSet

```rust
fn system(
    mut set: ParamSet<(
        Query<&mut Visibility, With<AppWindow>>,
        Query<&mut Visibility, With<DockTooltip>>,
    )>,
) {
    // 分时访问，不同时使用
    for v in set.p0().iter_mut() { ... }
    for v in set.p1().iter_mut() { ... }
}
```

---

## Bevy 0.19 API 速查

### 组件访问

| 操作 | API |
|------|-----|
| 设置光标图标 | `commands.entity().insert(CursorIcon::System(...))` |
| 移除光标图标 | `commands.entity().remove::<CursorIcon>()` |
| 设置可见性 | `commands.entity().insert(Visibility::Hidden)` |
| 获取父实体 | `query.get::<ChildOf>().get()` |

### 常用组件

```rust
// 圆角
Node {
    border_radius: BorderRadius::all(Val::Px(10.0)),
    // 或者分别设置
    border_radius: BorderRadius {
        top_left: Val::Px(10.0),
        top_right: Val::Px(10.0),
        bottom_left: Val::Px(0.0),
        bottom_right: Val::Px(0.0),
    },
}

// 溢出隐藏（圆角生效必需）
Node {
    overflow: Overflow::hidden(),
}

// 绝对定位
Node {
    position_type: PositionType::Absolute,
    top: Val::Px(32.0),
    left: Val::Px(100.0),
}
```

### 光标图标映射

```
ResizeEw    → 左右调整
ResizeNs    → 上下调整
ResizeNwSe  → 左上/右下调整
ResizeNeSw  → 右上/左下调整
Default     → 默认箭头
```

---

## 架构最佳实践

### 状态管理三层架构

```
┌─────────────────────────────────────────┐
│  业务状态层 (AppVisible, IsDragging)    │  ← 标记组件，安全查询
├─────────────────────────────────────────┤
│  命令层 (Commands)                       │  ← 延迟应用修改
├─────────────────────────────────────────┤
│  渲染状态层 (Visibility, Node)          │  ← Bevy 内部使用
└─────────────────────────────────────────┘
```

### 查询隔离原则

1. **同一组件不要被多个系统以不同模式查询**
2. **使用标记组件隔离业务状态和渲染状态**
3. **修改操作用 Commands，读取操作用 Query**
4. **需要立即读取修改结果时，合并到同一系统**

### 插件设计

```rust
pub struct MyPlugin;

impl Plugin for MyPlugin {
    fn build(&self, app: &mut App) {
        app
            // 资源
            .insert_resource(MyState::default())
            // 系统（注意避免冲突）
            .add_systems(Update, (
                system_a,
                system_b.after(system_a),  // 显式排序
            ));
    }
}
```

---

## 调试技巧

### 启用调试信息

```bash
# 看到具体系统名称
RUST_BACKTRACE=1 cargo run

# Bevy 调试特性
cargo run --features bevy/dynamic_linking
```

### B0001 排查步骤

1. 看错误信息中的系统名称（需要开启 debug feature）
2. 找到两个冲突的系统
3. 检查它们查询的组件类型
4. 选择解决方案：合并 / Commands / 标记组件

### 常见冲突组件

- `Visibility` (最常见)
- `Node` (布局)
- `Transform` (2D/3D)
- `BackgroundColor` (UI)
