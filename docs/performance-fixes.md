本次修改针对帧调度、剪贴板发送、多观看端编码和 Windows GPU 回读路径。

- 帧间隔从处理开始计算，编码耗时包含在预算内；发送和编码完成直接唤醒采集线程，替代固定 5 ms 轮询。
- 普通消息和视频使用同一个可交替接收的发送入口。剪贴板发送遇到背压时，连接仍能接收键鼠消息；保留单条消息在途、加密消息顺序、按显示器确认视频及原连接超时。
- 软件/HWRAM 编码器在观看端独立工作线程内创建、转换、编码、重建和销毁。每个工作线程最多一条任务在途，不积累过期帧；工作线程及退出清理中的线程合计上限为 32。
- 原尺寸观看端保留 GPU 纹理编码，缩放观看端按需共享像素回读，避免一个缩放连接使其他观看端整体降级。工作线程只接收有所有权的不可变像素快照；纹理和 GPU 编码器保持原有线程归属。
- DXGI 系统内存回读复用暂存纹理；设备重建、尺寸或格式变化后重新创建。切换采集路径时保持帧编号递增，防止工作线程误用旧 YUV 数据。

验证：

```powershell
build\local-tools\native.cmd test --locked --offline --lib --features flutter,hwcodec,vram server:: -- --test-threads=1
build\local-tools\native.cmd test --locked --offline -p scrap --features hwcodec,vram --lib texture_snapshot_readback_reuses_staging_and_preserves_pixels -- --test-threads=1
build\local-tools\native.cmd build --locked --offline --release --lib --features flutter,hwcodec,vram
```

新增测试覆盖帧预算、等待前到达的通知、阻塞的编码工作线程不影响另一观看端、退出连接不等待原生调用、共享帧快照不被覆盖、采集切换不复用旧帧编号，以及单项/多格式剪贴板协议下的输入接收、消息加密顺序和视频确认范围。原有静止画面补帧、分辨率缩放/解码、每连接一帧在途及恢复监控测试继续运行。

GPU 回读测试使用 Windows WARP 创建设备，实际检查复制前后像素与暂存资源复用；它不能替代 Intel QSV/NVIDIA/AMD 硬件编码测试。GPU 纹理编码仍在同一采集线程内执行，共享 GPU 饱和或驱动阻塞仍会影响该显示器。线程检测在受监督的 Windows 安装版中执行进程恢复，不会强行终止单个原生线程。

双机验收应覆盖：单端 1080p/4K 动态画面，两个不同分辨率观看端，慢链路下的大剪贴板同步，关闭慢观看端后继续操作，静止画面补帧，以及锁屏/长时间断开后的重连。尚未用真实双机测得 FPS、CPU 或延迟改善比例；发布 DLL 构建和替换本机安装文件是不同步骤。
