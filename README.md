# Enc Res

로봇 관절 엔코더 분해능 · 속도 양자화 계획 도구 (Rust → WebAssembly, 100 % 브라우저 내 실행).

## 한국어

### 무엇을 하나
엔코더 종류, 장착 위치, 기어비, 링크 길이, 제어 주기를 입력하면 다음을 계산합니다.
- 출력 1회전당 카운트, 출력 각도 분해능 (deg, arcsec, mrad)
- 카운트당 툴팁 이동 `L·Δθ` (µm), 백래시로 인한 툴팁 유격, 둘 중 무엇이 지배하는지
- M법(샘플당 카운트 차) 속도 양자 `Δθ·f_s`, 샘플당 1카운트를 얻는 최소 속도, 작업 속도에서의 오차와 통과/주의/실패 판정
- T법(주기 측정) 교차 속도 `Δθ·√(f_s·f_timer)`와 작업 속도에서의 에지 간격·오차
- 최대 속도에서의 카운트 주파수 대 디코더 한계 (80 % 초과 시 주의), 절대형은 샘플당 엔코더 회전 수

그래프: 실제 속도(로그) 대비 M법 측정 속도 계단 + 이상선, 목표 오차 대역, 작업 속도 표시, 상대 오차 % (로그-로그), 카운트 눈금이 보이도록 확대한 다이얼 인셋.

### 모델 / 수식
- 증분형: 카운트/회전 = 라인 × 디코딩(1, 2, 4). 절대형: 2^비트.
- 모터측 장착이면 출력 1회전당 카운트 = 엔코더 카운트 × N.
- Δθ = 360° / 출력 카운트, 팁 스텝 = L·Δθ(rad), 백래시 유격 = L·백래시(rad).
- M법 양자 q = Δθ·f_s, 최악 오차 = q·(1+√2·σ)/v (100 % 상한), 측정값 = floor(v/q)·q.
- T법 최악 오차 = v / (Δθ·f_timer).
- 에지 주파수 = 출력 회전/s × 출력 카운트.

### 한계
계획용 추정치입니다. 엔코더 정확도(INL, 편심), 축 비틀림, 감속기 전달 오차, 필터링(관측기/PLL), 지연은 모델링하지 않습니다. 실제로 측정하세요. 프리셋은 가상의 예시입니다.

### 조작
- 설정 카드(접기 가능)에서 값을 수정하면 즉시 갱신. 잘못된 값은 입력 아래에 메시지가 표시되고 마지막 유효 결과가 유지됩니다.
- 그래프 위 마우스 이동 또는 포커스 후 ←/→ (Shift = 크게), Home/End, Esc 로 값 읽기.
- "비교 고정"으로 현재 설계를 회색 고스트로 고정하고 차이(Δ 팁 스텝, Δ 최소 속도) 확인.
- 프리셋 3종, CSV 내보내기, KO/EN 전환. 모바일에서는 설정이 하단 시트로 열립니다.

### 로컬 빌드
```
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo test
trunk serve      # 또는 trunk build --release → dist/
bash tools/privacy_check.sh
```
앱은 `/enc-res/` 하위 경로에서 동작하도록 빌드됩니다.

### 개인정보
서버, 분석, 추적기, 외부 폰트/CDN, 브라우저 저장소, 런타임 네트워크 요청이 없습니다. 입력값은 브라우저 밖으로 나가지 않습니다.

### 라이선스
MIT — `LICENSE` 참조.

## English

### What it does
Given encoder type, mounting, gear ratio, lever arm and control rate, Enc Res computes:
- counts per output revolution and output angular resolution (deg, arcsec, mrad)
- tool-tip step per count `L·Δθ` (µm), backlash tip play, and which one dominates
- M-method (count difference per sample) velocity quantum `Δθ·f_s`, minimum speed for one count per sample, error at the working speed with a pass/warn/fail chip
- T-method (period measurement) crossover speed `Δθ·√(f_s·f_timer)` and edge period/error at the working speed
- count rate at max speed vs the decoder limit (warn above 80 %); for absolute encoders, encoder revolutions per sample

Plot: M-method staircase vs the ideal line over log true speed, target-error band, working-speed marker, relative error % (log-log), and a dial inset zoomed until individual counts are visible.

### Model / formulas
- Incremental: counts/rev = lines × decoding (1, 2, 4). Absolute: 2^bits.
- Motor-side mounting multiplies counts per output revolution by N.
- Δθ = 360° / output counts; tip step = L·Δθ(rad); backlash play = L·backlash(rad).
- M-method quantum q = Δθ·f_s; worst-case error = q·(1+√2·σ)/v (capped at 100 %); reading = floor(v/q)·q.
- T-method worst-case error = v / (Δθ·f_timer).
- Edge frequency = output rev/s × output counts.

### Limitations
Planning estimate only. Encoder accuracy (INL, eccentricity), shaft wind-up, gearbox transmission error, filtering (observers/PLL) and latency are not modelled; measure. Presets are synthetic and illustrative.

### Controls
- Edit the collapsible settings cards; results update live. Invalid values show an inline message and the last valid result stays.
- Hover the plot, or focus it and use ←/→ (Shift = bigger steps), Home/End, Esc.
- "Pin" keeps the current design as a grey ghost and shows deltas (Δ tip step, Δ min speed).
- Three presets, CSV export, KO/EN toggle. On mobile the settings open as a bottom sheet.

### Local build
```
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo test
trunk serve      # or trunk build --release -> dist/
bash tools/privacy_check.sh
```
The build targets the `/enc-res/` subpath.

### Privacy
No server, analytics, trackers, external fonts/CDNs, browser storage or runtime network requests. Entered values never leave the browser.

### License
MIT — see `LICENSE`.
