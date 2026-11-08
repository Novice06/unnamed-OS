#pragma once

extern void TASK_create_idle_process();
extern void TASK_init_idle_thread();
extern void TASK_init_scheduler();

extern void TASK_spawn_thread(void* entry_point);
extern void TASK_exit_thread(int code);
extern void TASK_yield();