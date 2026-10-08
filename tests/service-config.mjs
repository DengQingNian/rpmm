import assert from "node:assert/strict";
import { applyService, readService, serviceDefaults, serviceText } from "../ui/serviceConfig.ts";

const original = `[Unit]\nAfter="db.service" \\\n# 续行内的注释应保留\n cache.service\nRequires=db.service\n[Service]\nExecStart=C:/app.exe\nStandardOutputDirectory=D:/logs/%n\nMemoryMax=2G\nCPUQuota=50%\n[X-Meta]\nAfter=保持元数据\n`;
const draft = readService(original);
assert.deepEqual(draft.after, ["db.service", "cache.service"]);
assert.equal(draft.memoryMax, 2048);
assert.equal(draft.cpuQuota, 50);
assert.equal(applyService(original, draft), original);

// 只修改资源额度时，必须保留排序语义和动态路径，避免表单影响其他设置。
draft.cpuQuota = 25;
const resourceEdit = applyService(original, draft);
assert.ok(resourceEdit.includes('After="db.service" \\\n'));
assert.ok(resourceEdit.includes("StandardOutputDirectory=D:/logs/%n"));
assert.equal((resourceEdit.match(/Requires=/g) ?? []).length, 1);
assert.equal(readService(resourceEdit).cpuQuota, 25);

draft.after = ["queue.service"];
const dependencyEdit = applyService(original, draft);
assert.ok(dependencyEdit.includes("# 续行内的注释应保留"));
assert.ok(dependencyEdit.includes("[X-Meta]\nAfter=保持元数据"));
assert.ok(!dependencyEdit.includes(" cache.service"));
assert.deepEqual(readService(dependencyEdit).after, ["queue.service"]);
assert.deepEqual(readService(dependencyEdit).requires, ["db.service", "queue.service"]);

const create = serviceDefaults();
create.after = ["db.service"];
create.healthAfter = ["db.service"];
create.stdoutDirectory = "D:\\100% output";
create.memoryMax = 512;
create.cpuQuota = 10;
const generated = serviceText(create);
assert.ok(generated.includes("StandardOutputDirectory=D:/100%% output"));
assert.equal(readService(generated).stdoutDirectory, "D:/100% output");
assert.deepEqual(readService(generated + "\n[Unit]\nHealthAfter=\n").healthAfter, []);
console.log("服务表单验证通过：续行与注释、局部更新、依赖语义、路径转义和额度。 ");
