---
name: os_navigation
description: Autonomous Windows desktop navigation, UI inspection, mouse clicking, and keyboard typing.
version: 3.1.0
---

### Windows Desktop Navigation Runbook:
1. **Target Identification**:
   - Target 1 (Active Window / Desktop): UI controls and document areas with sequential Tag IDs `[1]` to `[K]`.
   - Target 2 (Windows Taskbar): Pinned applications (Edge, Chrome, File Explorer, Terminal), active app switchers, and Start menu with Tag IDs `[K+1]` to `[M]`.
2. **Launching Applications**:
   - To open an application pinned on the taskbar, click its taskbar button using `click_element(tag_id: <id>)`.
   - Windows launches the app upon click. Conclude immediately with `tool: task_completed` rather than repeating the click.
   - If an application is not on the taskbar, click Start or use `type_text(text: "<app_name>", press_enter: true)` to search and launch.
3. **Multi-Window Navigation**:
   - To switch between open applications, click the corresponding taskbar button or window title.
