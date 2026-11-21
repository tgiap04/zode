---
title: REPL - Jupyter Kernels in Zode
description: Run code interactively in Zode with built-in Jupyter kernel support. Execute Python, TypeScript, R, and more inline.
---

# REPL

## Bắt đầu

REPL tích hợp sẵn của Zode dùng [Jupyter kernel](https://docs.jupyter.org/en/latest/projects/kernels.html) để bạn chạy code tương tác ngay trong các file soạn thảo thông thường.

<figure style="width: 100%; margin: 0; overflow: hidden; border-top-left-radius: 2px; border-top-right-radius: 2px;">
    <video loop controls playsinline>
        <source
            src="https://customer-snccc0j9v3kfzkif.cloudflarestream.com/aec66e79f23d6d1a0bee5e388a3f17cc/downloads/default.mp4"
            type='video/webm; codecs="vp8.0, vorbis"'
        />
        <source
            src="https://customer-snccc0j9v3kfzkif.cloudflarestream.com/aec66e79f23d6d1a0bee5e388a3f17cc/downloads/default.mp4"
            type='video/mp4; codecs="avc1.4D401E, mp4a.40.2"'
        />
        <source
          src="https://zed.dev/img/post/repl/typescript-deno-kernel-markdown.png"
          type="image/png"
        />
    </video>
</figure>

## Cài đặt

Zode hỗ trợ chạy code bằng nhiều ngôn ngữ. Để bắt đầu, bạn cần cài kernel cho ngôn ngữ muốn dùng.

**Các ngôn ngữ hiện được hỗ trợ:**

- [Python (ipykernel)](#python)
- [TypeScript (Deno)](#typescript-deno)
- [R (Ark)](#r-ark)
- [R (Xeus)](#r-xeus)
- [Julia](#julia)
- [Scala (Almond)](#scala)

Sau khi cài, bạn có thể dùng REPL trong các file của ngôn ngữ tương ứng, hoặc những nơi khác có hỗ trợ ngôn ngữ đó, chẳng hạn Markdown. Nếu vừa thêm kernel, hãy chạy lệnh `repl: refresh kernelspecs` để chúng xuất hiện trong trình soạn thảo.

## Dùng REPL

Để khởi động REPL, mở file của ngôn ngữ bạn muốn dùng và chạy lệnh `repl: run` (mặc định là `ctrl-shift-enter` trên macOS) để chạy một khối, vùng chọn hoặc một dòng. Bạn cũng có thể nhấn biểu tượng REPL trên thanh công cụ.

Lệnh `repl: run` sẽ chạy trên vùng chọn của bạn, và kết quả hiện bên dưới vùng chọn.

Có thể xóa kết quả bằng lệnh `repl: clear outputs`, hoặc từ menu REPL trên thanh công cụ.

### Chế độ cell

Zode hỗ trợ [notebook dạng script](https://jupytext.readthedocs.io/en/latest/formats-scripts.html) với dấu phân cách cell `# %%` trong Python và `// %%` trong TypeScript. Nhờ đó bạn viết code trong một file duy nhất và chạy như một notebook, từng cell một.

Lệnh `repl: run` sẽ chạy mỗi khối code giữa các dấu `# %%` như một cell riêng.

```python
# %% Cell 1
import time
import numpy as np

# %% Cell 2
import matplotlib.pyplot as plt
import matplotlib.pyplot as plt
from matplotlib import style
style.use('ggplot')
```

## Hướng dẫn theo từng ngôn ngữ

### Python {#python}

#### Môi trường global

<div class="warning">

Trên macOS, Python của hệ thống sẽ _không_ chạy được. Hãy cài [pyenv](https://github.com/pyenv/pyenv?tab=readme-ov-file#installation) hoặc dùng virtual environment.

</div>

Để Python hiện tại của bạn có sẵn một kernel, chạy:

```sh
pip install ipykernel
python -m ipykernel install --user
```

#### Môi trường Conda

```sh
source activate myenv
conda install ipykernel
python -m ipykernel install --user --name myenv --display-name "Python (myenv)"
```

#### Virtualenv với pip

```sh
source activate myenv
pip install ipykernel
python -m ipykernel install --user --name myenv --display-name "Python (myenv)"
```

### R (Ark Kernel) {#r-ark}

Cài [Ark](https://github.com/posit-dev/ark/releases) bằng cách tải bản phát hành cho hệ điều hành của bạn. Ví dụ với macOS, chỉ cần giải nén binary `ark` và đặt vào `/usr/local/bin`. Sau đó chạy:

```sh
ark --install
```

### R (Xeus Kernel) {#r-xeus}

- Cài [Xeus-R](https://github.com/jupyter-xeus/xeus-r)
- Cài extension R cho Zode (tìm `R` trong Zode Extensions)

<!--
TBD: Improve R REPL (Ark Kernel) instructions
-->

### TypeScript: Deno {#typescript-deno}

- [Cài Deno](https://docs.deno.com/runtime/manual/getting_started/installation/) rồi cài Deno jupyter kernel:

```sh
deno jupyter --install
```

<!--
TBD: Improve R REPL (Ark Kernel) instructions
-->

### Julia

- Tải và cài Julia từ [trang chính thức](https://julialang.org/downloads/).
- Cài extension Julia cho Zode (tìm `Julia` trong Zode Extensions)

<!--
TBD: Improve Julia REPL instructions
-->

### Scala

- [Cài Scala](https://www.scala-lang.org/download/) bằng `cs setup` (Coursier):
  - `brew install coursier/formulas/coursier && cs setup`
- REPL (Almond) [hướng dẫn thiết lập](https://almond.sh/docs/quick-start-install):
  - `brew install --cask temurin` (binary OpenJDK chính thức của Eclipse foundation)
  - `brew install coursier/formulas/coursier && cs setup`
  - `coursier launch --use-bootstrap almond -- --install`

## Đổi kernel dùng cho từng ngôn ngữ {#changing-kernels}

Zode tự phát hiện các kernel có sẵn và sắp xếp chúng trong bộ chọn kernel:

- **Recommended**: Môi trường Python khớp với toolchain đang hoạt động (nếu phát hiện được)
- **Python Environments**: Virtual environment (venv, virtualenv, Poetry, Pipenv, Conda, uv, v.v.)
- **Jupyter Kernels**: Các kernelspec Jupyter đã cài
- **Remote Servers**: Các Jupyter server từ xa đã kết nối

### Cài ipykernel

Môi trường Python vẫn hiện trong bộ chọn dù chưa cài ipykernel. Môi trường thiếu ipykernel bị làm mờ và gắn nhãn "ipykernel not installed." Khi bạn chọn một môi trường như vậy, Zode tự chạy `pip install ipykernel` trong môi trường đó và kích hoạt nó khi cài xong.

### Zode đề xuất kernel như thế nào

Khi bạn chạy code, Zode tự chọn kernel:

1. **Khớp toolchain đang hoạt động**: Nếu một môi trường Python khớp với toolchain đang hoạt động và có ipykernel, Zode dùng nó
2. **Môi trường Python đầu tiên có sẵn**: Nếu không, dùng môi trường Python đầu tiên có ipykernel
3. **Dự phòng theo ngôn ngữ**: Nếu không có môi trường Python nào sẵn sàng, Zode chọn Jupyter kernel khớp với ngôn ngữ của khối code

Bạn có thể ghi đè bằng cách tự chọn kernel trong bộ chọn.

### Đặt kernel mặc định

Để cấu hình kernel mặc định khác cho một ngôn ngữ, bạn có thể gán kernel cho bất kỳ ngôn ngữ nào được hỗ trợ trong `settings.json`:

```json [settings]
{
  "jupyter": {
    "kernel_selections": {
      "python": "conda-env",
      "typescript": "deno",
      "javascript": "deno",
      "r": "ark"
    }
  }
}
```

## Nhập liệu tương tác

Khi code cần người dùng nhập (như hàm `input()` của Python), REPL hiện ô nhập bên dưới kết quả của cell.

Gõ câu trả lời vào ô văn bản và nhấn `Enter` để gửi. Kernel nhận dữ liệu của bạn và chạy tiếp.

Với ô nhập mật khẩu, ký tự được che bằng dấu sao để bảo mật.

Nếu việc chạy bị ngắt khi ô nhập đang hoạt động, ô nhập tự xóa khi kernel trở về trạng thái rảnh.

## Gỡ lỗi kernelspec

Các kernel có sẵn hiện qua lệnh `repl: sessions`. Để làm mới danh sách kernel có thể chạy, dùng lệnh `repl: refresh kernelspecs`.

Nếu đã cài `jupyter`, bạn có thể chạy `jupyter kernelspec list` để xem các kernel có sẵn.

```sh
$ jupyter kernelspec list
Available kernels:
  ark                   /Users/z/Library/Jupyter/kernels/ark
  conda-base            /Users/z/Library/Jupyter/kernels/conda-base
  deno                  /Users/z/Library/Jupyter/kernels/deno
  python-chatlab-dev    /Users/z/Library/Jupyter/kernels/python-chatlab-dev
  python3               /Users/z/Library/Jupyter/kernels/python3
  ruby                  /Users/z/Library/Jupyter/kernels/ruby
  rust                  /Users/z/Library/Jupyter/kernels/rust
```

> Lưu ý: Zode cố gắng hết sức dùng `sys.prefix` và `CONDA_PREFIX` để tìm kernel trong các môi trường Python. Nếu muốn kiểm soát rõ ràng, chạy `python -m ipykernel install --user --name myenv --display-name "Python (myenv)"` để cài kernel trực tiếp khi đang ở trong môi trường đó.
