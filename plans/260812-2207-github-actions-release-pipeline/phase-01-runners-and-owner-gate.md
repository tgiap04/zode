# Phase 01 — Runner + cổng owner: workflow chạy được trên account này

**Priority:** P1 · **Status:** ⬜ pending · **Blocked by:** — · **Chạy song song với:** [Phase 02](phase-02-scripts-signing-and-remote-server.md)
**Context:** [plan.md](plan.md) · [brainstorm 260819](./reports/brainstorm-260819-release-pipeline-public-repo.md)

## Mục tiêu

Sau phase này, `cargo xtask workflows` sinh ra YAML mà GitHub **có thể nhận job** trên `tgiap04/zode`: runner tồn tại, cổng owner không skip, và không action nào đòi hạ tầng Namespace.

Phase này **chưa** cần build thành công — nó chỉ cần job rời khỏi queue và bắt đầu chạy.

## Key insight

60 chỗ trong `tooling/xtask/**` tham chiếu hằng số runner, nhưng **không chỗ nào hardcode chuỗi runner**. Đổi 15 dòng trong `runners.rs` là đổi runner cho toàn bộ repo. Tương tự, cổng owner là **một** hằng số ở `steps.rs:325` dùng chung cho cả 34 workflow.

Đây là lý do phase này nhỏ mà ăn rộng. Không đi sửa từng workflow.

## Related code files

**Modify:**
- `tooling/xtask/src/tasks/workflows/runners.rs` — hằng số runner (dòng 1-15) + hàm chọn runner theo arch
- `tooling/xtask/src/tasks/workflows/steps.rs:325` — `DEFAULT_REPOSITORY_OWNER_GUARD`
- `tooling/xtask/src/tasks/workflows/steps.rs:236-244` — `cache_rust_dependencies_namespace()`
- `tooling/xtask/src/tasks/workflows/run_bundling.rs:96,189` — chọn runner theo arch cho mac/windows
- `.github/workflows/*.yml` — **file sinh ra**, commit kèm, không sửa tay

**Không tạo file mới.**

## Implementation steps

1. **`runners.rs` — map hằng số sang GitHub-hosted.**

   ```rust
   pub const LINUX_SMALL: Runner = Runner("ubuntu-24.04");
   pub const LINUX_DEFAULT: Runner = LINUX_XL;
   pub const LINUX_XL: Runner = Runner("ubuntu-24.04");
   pub const LINUX_LARGE: Runner = Runner("ubuntu-24.04");
   pub const LINUX_MEDIUM: Runner = Runner("ubuntu-24.04");
   pub const LINUX_LARGE_RAM: Runner = Runner("ubuntu-24.04");

   // 22.04 = glibc 2.35, sàn thấp nhất GitHub còn cung cấp.
   // Upstream cố ý dùng 20.04 (glibc 2.31); runner đó đã bị GitHub gỡ.
   pub const LINUX_X86_BUNDLER: Runner = Runner("ubuntu-22.04");
   pub const LINUX_ARM_BUNDLER: Runner = Runner("ubuntu-22.04-arm");

   pub const MAC_DEFAULT: Runner = Runner("macos-15");
   pub const MAC_INTEL: Runner = Runner("macos-15-intel");
   pub const WINDOWS_DEFAULT: Runner = Runner("windows-2025");
   pub const WINDOWS_ARM: Runner = Runner("windows-11-arm");
   ```

   Bốn tier Linux gộp về cùng một label là **có ý**: runner free chỉ có một cỡ (4 vCPU / 16 GB), giữ bốn tên khác nhau trỏ cùng chỗ vẫn tốt hơn xoá tên vì 60 call-site không phải đổi.

2. **`runners.rs` — thêm hàm chọn runner theo arch**, đối xứng với `Arch::linux_bundler()` đang có:

   ```rust
   impl Arch {
       pub fn mac_bundler(&self) -> Runner {
           match self { Arch::AARCH64 => MAC_DEFAULT, Arch::X86_64 => MAC_INTEL }
       }
       pub fn windows_bundler(&self) -> Runner {
           match self { Arch::AARCH64 => WINDOWS_ARM, Arch::X86_64 => WINDOWS_DEFAULT }
       }
   }
   ```

3. **`run_bundling.rs` — dùng hai hàm đó.** Hiện `bundle_mac` (`:96`) và `bundle_windows` (`:189`) hardcode `runners::MAC_DEFAULT` / `runners::WINDOWS_DEFAULT` **bất kể arch** — đúng với Zed (một mac-large build cả hai arch) nhưng sai với runner free. Đổi thành `arch.mac_bundler()` / `arch.windows_bundler()`.

4. **`steps.rs:325` — mở cổng owner:**

   ```rust
   const DEFAULT_REPOSITORY_OWNER_GUARD: &str = "(github.repository_owner == 'tgiap04')";
   ```

   Giữ cơ chế guard thay vì xoá: nó vẫn chặn workflow chạy trên fork của người khác. Xoá đi là mời fork lạ đốt quota.

5. **`steps.rs:236-244` — thay `nscloud-cache-action` bằng `actions/cache`.** Action của Namespace cần volume cache của Namespace; trên runner GitHub nó **fail**, không phải degrade. Đổi *thân hàm*, giữ nguyên tên và 10 call-site:

   ```rust
   pub fn cache_rust_dependencies_namespace() -> Step<Use> {
       named::uses("actions", "cache", "<pin sha v4>")
           .add_with(("path", "~/.cargo/registry\n~/.cargo/git"))
           .add_with(("key", "cargo-${{ runner.os }}-${{ runner.arch }}-${{ hashFiles('**/Cargo.lock') }}"))
           .add_with(("restore-keys", "cargo-${{ runner.os }}-${{ runner.arch }}-"))
       }
   ```

   Đổi tên hàm cho khỏi nói dối là việc dọn tuỳ chọn, làm sau cũng được — đừng để nó nở thành 10 chỗ sửa trong phase này.

   **Chỉ cache registry + git, KHÔNG cache `target/`.** Trần cache 10 GB/repo; `target/` release lớn hơn nhiều lần và sẽ thrash.

6. **`cargo xtask workflows`** rồi commit YAML sinh ra. Bỏ bước này là gate `run_tests.yml:705` đỏ.

## Todo

- [ ] `runners.rs`: map 12 hằng số sang runner GitHub-hosted
- [ ] `runners.rs`: thêm `Arch::mac_bundler()` + `Arch::windows_bundler()`
- [ ] `run_bundling.rs:96,189`: dùng runner theo arch
- [ ] `steps.rs:325`: đổi owner guard sang `tgiap04`
- [ ] `steps.rs:236`: `nscloud-cache-action` → `actions/cache` (registry + git, không target)
- [ ] `cargo build -p xtask` xanh
- [ ] `cargo xtask workflows`; commit `.github/workflows/**`
- [ ] Kiểm bằng mắt: `grep -c "namespace-profile\|self-32vcpu\|nscloud" .github/workflows/` = 0
- [ ] Kiểm bằng mắt: `grep -c "zed-industries" .github/workflows/release.yml` chỉ còn ở chỗ Phase 04 sẽ xử lý

## Success criteria

- `cargo xtask workflows` chạy lại không sinh diff (gate sync xanh).
- Không còn chuỗi `namespace-profile-`, `self-32vcpu-`, `nscloud-cache-action` trong `.github/workflows/`.
- Push nhánh này lên GitHub: một job của `run_tests` **bắt đầu chạy** (không còn "waiting for a runner"). Job được phép fail — phase này chỉ cần nó rời queue.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| `ubuntu-22.04-arm` / `windows-11-arm` chỉ tồn tại trên **public repo**. Repo về private là hai target này chết. | Ghi vào README. Không có đối phó kỹ thuật — đó là ràng buộc của quyết định #1. |
| Gộp 4 tier Linux về một label làm job test nặng chạy trên máy nhỏ hơn upstream dự tính → có thể timeout | Trần `timeout_minutes(60)` ở `steps.rs` có thể phải nâng. Xử ở Phase 03, không đoán trước. |
| `actions/cache` sai key → cache miss âm thầm, không ai biết | Đọc dòng "Cache restored from key" trong log job đầu tiên. |
| Đổi thân `cache_rust_dependencies_namespace` ảnh hưởng **cả workflow không thuộc plan này** (autofix_pr, compliance_check, run_tests) | Đúng, và đó là điều mong muốn — chúng vốn đã hỏng trên account này. Đừng thêm nhánh điều kiện. |

## Security considerations

- Giữ owner guard thay vì xoá: chặn workflow tự chạy trên fork của người khác.
- Không thêm secret nào ở phase này.
- `actions/cache` là cache **công khai đọc được** trên public repo — chỉ chứa crate registry, không chứa gì bí mật. Không bao giờ cache thứ có credential.

## Next steps

→ [Phase 03](phase-03-fit-the-build-on-free-runners.md) (cần cả 01 và 02 xong)
