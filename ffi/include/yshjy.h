/* yshjy C ABI v1 —— 杨三海解压引擎（iOS / Android / 其它外壳）
 *
 * 约定：
 *   - 入参出参都是 UTF-8 JSON（不是 C 字符串以外的东西）
 *   - 本库返回的每个 char* 都必须用 yshjy_string_free() 释放
 *   - 事件回调在引擎工作线程上触发；回调里不要做长时间阻塞操作
 *   - 同一 Job 只能 free 一次；free 会先取消并等待 worker 退出
 */
#ifndef YSHJY_H
#define YSHJY_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define YSHJY_ABI_VERSION 1

typedef void (*yshjy_event_cb)(const char *json, size_t len, void *user);
typedef struct YshjyJob YshjyJob;

uint32_t yshjy_abi_version(void);
char *yshjy_version(void);
char *yshjy_hello(void);

/* 同步执行：{"cmd":"list","path":"/x.mp4"} → {"ok":true,"result":{...}} */
char *yshjy_request(const char *json);

/* 异步执行：立即返回句柄；事件走 cb；结果用 wait 取 */
YshjyJob *yshjy_job_submit(const char *json, yshjy_event_cb cb, void *user);
int32_t yshjy_job_cancel(YshjyJob *job);
char *yshjy_job_wait(YshjyJob *job); /* 阻塞；可重复调用 */
void yshjy_job_free(YshjyJob *job);

void yshjy_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif /* YSHJY_H */
