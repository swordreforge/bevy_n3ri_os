# 窗口手势隔离：一次三层缺陷的排查记录

> 日期：2026-08-25 · 涉及模块：`n3ri-ui/src/{window.rs, resize.rs}` · 修复提交：`d1c7fb6`, `011b197`

## 症状

多窗口场景下两类异常：

1. **移动窗口 A，窗口 B 的宽高被拉伸**（用户最初报告"UI 缩放没有隔离"）
2. **竞态修复后，缩放光标在停止操作后仍然生效**

## 根因链：一个症状，三个断层

手势系统的生命周期是 `start → apply → end`。排查发现每一环都有独立缺陷：

### 断层 1：触发层——共享输入事件无互斥

```rust
// resize_start 和 window_drag_start 都是这个模式：
if !mouse.just_pressed(MouseButton::Left) {
    return;
}
// ...没有任何共享状态检查，直接开始各自的手势
```

两个系统监听同一原始事件且互不知晓。拖拽窗口 A 的同一帧，如果光标恰好落在
窗口 B 边缘 8px 内，`resize_start` 会同时对 B 启动缩放——之后 `resize_apply`
改 B 的 width/height，`window_drag_apply` 改 A 的 left/top，同时生效。

**修复**（`d1c7fb6`）：入口处检查共享的 `IsDragging` 资源：

```rust
if is_dragging.0 {
    return;
}
```

两个系统都先查后写。Bevy 对 `ResMut<IsDragging>` 的冲突访问自动串行化，
因此后执行的系统必然看到先执行者写入的 `true`。

### 断层 2：隔离层——命中检测不排除隐藏窗口

最小化窗口只是 `Visibility::Hidden` + `AppVisible(false)`，其 `Node`
坐标数据照常存在。而边缘检测直接遍历所有 `AppWindow`：

```rust
for (entity, node) in window_query.iter() {   // 没有可见性过滤！
    let (left, top, width, height) = node_bounds(node);
    let edge = detect_edge(cursor, left, top, width, height);
```

结果：鼠标扫过已最小化窗口残留的"幽灵边缘"，会显示缩放光标、甚至启动对
不可见窗口的缩放。这就是"没动也在缩放"的直接原因。

**修复**（`011b197`）：查询加入 `&Visibility` 并跳过非 Visible 实体。

### 断层 3：清理层——ECS 副作用不随 Resource 重置回滚

```rust
// resize_end 原实现：
if resize_state.resizing_window.is_some() {
    *resize_state = ResizeState::default();   // 只清了 Rust 内存
    is_dragging.0 = false;
    // OS 窗口上的 CursorIcon 组件没人管！
}
```

`ResizeState` 是系统私有的 Resource，但手势过程中往 OS 窗口实体上 insert
过 `CursorIcon::System(NwseResize)` 这类组件。Resource 归零不会撤销组件
写入，光标就此卡死。

**修复**：`resize_end` 显式 `remove::<CursorIcon>()`；光标离开 OS 窗口
时同样清除。

## 提炼的原则

1. **共享输入事件的系统必须显式互斥。** ECS 调度器只保证访问冲突时的串行，
   不阻止多个系统响应同一帧同一按键。凡多系统监听同一原始事件，入口必须有
   共享状态守卫。

2. **有 insert 必有配对的 remove 路径。** 手势期间写到实体上的任何组件，
   都是必须归还的副作用。清理逻辑要覆盖所有出口：正常释放、光标离窗、
   窗口被关闭。

3. **查询即命中检测，过滤条件就是隔离边界。** Bevy UI 里"隐藏"只是渲染
   不可见，组件数据照常参与查询。做 hit-test 时自问：是否排除了不可见、
   已最小化、已关闭的对象？

4. **一个症状可能有多个根因。** 本例修掉竞态后症状变成"光标卡住"——不是
   回归，是下一层缺陷暴露了。修复后症状未消失，应继续分层排查而非认为
   修错了方向。

5. **用户的直觉诊断值得持续对照。** 用户首报即指出"没有隔离"。第一轮修复
   解决的是并发问题而非隔离问题，直到第二轮才兑现这个判断。若首轮修复后
   症状仍在，回头重审用户最初的假设。

## 手势系统 checklist

新增任何拖拽/缩放类交互时逐项确认：

- [ ] 与既有手势系统是否有共享状态互斥（同帧只能激活一个）
- [ ] hit-test 查询是否过滤了 `Visibility` 和其他"应失效"状态
- [ ] start 写入的每个组件/资源，end 是否都有对应清理
- [ ] 光标/预览等 UI 反馈在手势结束、光标离窗两条路径上都回收
- [ ] 最小值约束（如 MIN_WINDOW_WIDTH）在手势全程成立
