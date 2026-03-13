# ANP Demo Guide — 완전 초보자용 데모 시연 가이드

이 문서는 ANP(AI Native Programming)의 모든 데모를 **처음부터 끝까지** 따라할 수 있는 완전 상세 가이드입니다.

---

## 0. 사전 준비 (Prerequisites)

### 필수: Rust 설치

Rust가 설치되어 있지 않다면:

```bash
# Rust 설치 (1분 소요)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 설치 후 환경변수 로드
source $HOME/.cargo/env

# 설치 확인
rustc --version   # rustc 1.75.0 이상이면 OK
cargo --version   # cargo 1.75.0 이상이면 OK
```

### 필수: 프로젝트 클론 & 빌드

```bash
# 1. 프로젝트 다운로드
git clone https://github.com/dbwls99706/ANP.git
cd ANP

# 2. 빌드 (첫 빌드는 1~2분 소요)
cargo build

# 성공하면 이런 메시지가 나옴:
#   Compiling anp-core v0.1.0
#   Compiling anp-nodes v0.1.0
#   Compiling anp-planner v0.1.0
#   Compiling anp-cli v0.1.0
#   Finished `dev` profile [unoptimized + debuginfo] target(s)
```

### 선택: AI 기능을 쓰려면 API 키 설정

AI 관련 데모(챗봇, 에이전트, 스마트 API)를 실행하려면 **둘 중 하나** 설정:

```bash
# 방법 A: OpenAI API 키
export OPENAI_API_KEY=sk-xxxxxxxxxxxxxxxxxxxxxxxx

# 방법 B: Anthropic API 키
export ANTHROPIC_API_KEY=sk-ant-xxxxxxxxxxxxxxxxxxxxxxxx
```

> API 키가 없어도 **데모 1~4**는 완벽하게 동작합니다!

---

## 데모 1: Hello World — 파일 읽고 출력 (API 키 불필요)

**이게 뭐냐:** 파일을 읽어서 화면에 출력하는 가장 단순한 그래프.

**그래프 구조:**
```
file_read → console
```

### 실행

```bash
cargo run -p anp-cli -- run examples/hello.anp.json
```

### 예상 출력

```
 INFO starting ANP engine name=Hello ANP
 INFO execution plan created stages=2 entry_nodes=["source"]
 INFO starting batch execution run_id=...
Hello, ANP!
 INFO batch execution completed run_id=... elapsed_ms=0 nodes_executed=2 nodes_failed=0
 INFO ANP engine finished
```

### 핵심 포인트
- `nodes_executed=2` → source(file_read) + sink(console) 2개 노드가 실행됨
- `nodes_failed=0` → 에러 없이 성공
- `elapsed_ms=0` → 1밀리초도 안 걸림 (빠름!)
- **소스 코드 0줄.** 오직 JSON 그래프 하나로 동작.

---

## 데모 2: 데이터 파이프라인 — 읽기→변환→필터→출력 (API 키 불필요)

**이게 뭐냐:** JSON 데이터를 읽고, 필드를 추출하고, 조건으로 필터링하고, 포맷해서 출력.

**그래프 구조:**
```
file_read → transform → filter (score >= 80) → template → console
```

### 실행

```bash
cargo run -p anp-cli -- run examples/data_pipeline.anp.json
```

### 예상 출력

```
 INFO starting ANP engine name=AI Data Pipeline
 INFO execution plan created stages=5 entry_nodes=["source"]
 INFO starting batch execution run_id=...
[pipeline] Top performer: Alice (Engineering) — Score: 95
[pipeline] Top performer: Charlie (Design) — Score: 88
[pipeline] Top performer: Eve (Engineering) — Score: 91
 INFO batch execution completed run_id=... elapsed_ms=0 nodes_executed=5 nodes_failed=0
 INFO ANP engine finished
```

### 핵심 포인트
- 원본 데이터 5명 → score 80 이상만 필터 → 3명만 출력
- `stages=5` → 5개 노드가 순차적 파이프라인으로 실행
- `{{name}}`, `{{department}}`, `{{score}}` 템플릿이 자동으로 값으로 치환됨
- Bob(72점), Diana(64점)은 필터에 의해 제거됨

### 원본 데이터 확인 (궁금하면)

```bash
cat examples/sample_data.json
```

```json
[
  { "name": "Alice", "email": "alice@example.com", "score": 95, "department": "Engineering" },
  { "name": "Bob", "email": "bob@example.com", "score": 72, "department": "Marketing" },
  { "name": "Charlie", "email": "charlie@example.com", "score": 88, "department": "Design" },
  { "name": "Diana", "email": "diana@example.com", "score": 64, "department": "Sales" },
  { "name": "Eve", "email": "eve@example.com", "score": 91, "department": "Engineering" }
]
```

---

## 데모 3: 중첩 필드 추출 파이프라인 (API 키 불필요)

**이게 뭐냐:** 중첩된(nested) JSON에서 dot-path로 필드를 추출하고 조건 필터링.

**그래프 구조:**
```
file_read → transform (address.city 추출) → filter (age >= 20) → console
```

### 실행

```bash
cargo run -p anp-cli -- run examples/pipeline.anp.json
```

### 예상 출력

```
 INFO starting ANP engine name=Data Pipeline
 INFO execution plan created stages=4 entry_nodes=["reader"]
 INFO starting batch execution run_id=...
[result] [
  {
    "age": 25,
    "city": "Seoul",
    "name": "Alice"
  },
  {
    "age": 30,
    "city": "Incheon",
    "name": "Charlie"
  }
]
 INFO batch execution completed run_id=... elapsed_ms=0 nodes_executed=4 nodes_failed=0
 INFO ANP engine finished
```

### 핵심 포인트
- 원본: `{"address": {"city": "Seoul"}}` → transform이 `address.city`를 `city`로 **dot-path 추출**
- 4명 중 age >= 20인 Alice(25), Charlie(30)만 통과 (Bob 17, Diana 19 제외)

---

## 데모 4: REST API 서버 — HTTP 서버 + 인메모리 DB (API 키 불필요)

**이게 뭐냐:** JSON 파일 없이 HTTP 서버가 떠서 REST API로 데이터를 CRUD하는 서비스.

**그래프 구조:**
```
http_server(:3000) → database (in-memory) → console
```

### 실행 — 터미널 2개 필요!

**터미널 1: 서버 시작**

```bash
cargo run -p anp-cli -- run examples/simple_api.anp.json
```

출력:
```
 INFO starting ANP engine name=Simple REST API
 INFO execution plan created stages=3 entry_nodes=["server"]
 INFO HTTP server listening port=3000
```

서버가 떠서 대기 중입니다. **이 터미널은 그대로 두세요.**

**터미널 2: API 요청 보내기**

새 터미널을 열고 아래 명령을 하나씩 실행:

```bash
# 1. 헬스 체크
curl http://localhost:3000/health
# 예상: 404 또는 빈 응답 (health 라우트는 DB에 연결 안 됨)

# 2. 유저 목록 조회 (GET) — 초기 데이터 2명이 들어있음
curl http://localhost:3000/users
# 예상: {"op":"list","keys":["user:1","user:2"],"count":2} 또는 유사한 응답

# 3. 새 유저 추가 (POST)
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op": "set", "key": "user:3", "value": {"name": "Charlie", "age": 35}}'
# 예상: {"op":"set","key":"user:3","success":true}

# 4. 추가한 유저 조회
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op": "get", "key": "user:3"}'
# 예상: {"op":"get","key":"user:3","value":{"name":"Charlie","age":35},"found":true}

# 5. 전체 키 목록
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op": "list"}'
# 예상: {"op":"list","keys":["user:1","user:2","user:3"],"count":3}
```

**서버 종료:** 터미널 1에서 `Ctrl+C`

### 핵심 포인트
- **Service 모드**: http_server는 장시간 실행 노드로 요청을 기다림
- 코드 한 줄 없이 REST API가 동작
- 초기 데이터(`user:1`, `user:2`)는 database 노드의 config에 정의
- 매 요청마다 `[api]` 접두사로 콘솔에 로그 출력

---

## 데모 5: AI 챗봇 — LLM이 답변하는 HTTP 챗봇 (API 키 필요!)

**이게 뭐냐:** HTTP 요청을 받으면 LLM(GPT-4o)이 답변하는 챗봇.

**그래프 구조:**
```
http_server(:3000) → template → llm_call(GPT-4o) → console
```

### 사전 확인

```bash
# API 키가 설정되어 있는지 확인
echo $OPENAI_API_KEY
# sk-... 가 출력되어야 함. 비어있으면:
export OPENAI_API_KEY=sk-여기에키입력
```

### 실행 — 터미널 2개 필요!

**터미널 1: 서버 시작**

```bash
cargo run -p anp-cli -- run examples/ai_chatbot.anp.json
```

출력:
```
 INFO starting ANP engine name=AI Chatbot
 INFO HTTP server listening port=3000
```

**터미널 2: 챗봇에게 말 걸기**

```bash
# 질문 1
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "What is AI Native Programming?"}'
# 예상: LLM이 AI Native Programming에 대해 2~3문장으로 답변

# 질문 2
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "Explain quantum computing in simple terms"}'
# 예상: 양자 컴퓨팅에 대한 간결한 설명

# 질문 3 (한국어도 됨!)
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "프로그래밍이란 무엇인가요?"}'
# 예상: 한국어로 답변
```

**서버 종료:** `Ctrl+C`

### 핵심 포인트
- `template` 노드가 `{{message}}`를 추출해서 LLM에 전달
- `llm_call` 노드가 OpenAI API를 호출하여 답변 생성
- **"AI calling AI"** — 그래프 안에서 AI가 AI를 호출하는 구조
- 시스템 프롬프트: "You are a helpful, concise assistant. Answer in 2-3 sentences max."

---

## 데모 6: 스마트 API — REST + AI 분석 엔드포인트 (API 키 필요!)

**이게 뭐냐:** 일반 REST API에 AI 분석 기능이 결합된 서비스.

**그래프 구조:**
```
http_server(:3000) ─┬─ /items (POST) ──→ database → console
                    ├─ /items (GET)  ──→ database → console
                    └─ /analyze (POST) ──→ llm_call → console
```

### 실행 — 터미널 2개 필요!

**터미널 1:**

```bash
cargo run -p anp-cli -- run examples/smart_api.anp.json
```

**터미널 2:**

```bash
# 1. 아이템 저장
curl -X POST http://localhost:3000/items \
  -H "Content-Type: application/json" \
  -d '{"op": "set", "key": "item:1", "value": {"name": "Laptop", "price": 999}}'

# 2. 아이템 목록 조회
curl http://localhost:3000/items

# 3. AI 분석 요청 (이게 핵심!)
curl -X POST http://localhost:3000/analyze \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Analyze this customer feedback: Great product but shipping was slow"}'
# 예상: {"sentiment": "mixed", "category": "logistics", "summary": "..."}

# 4. 한국어 분석도 가능
curl -X POST http://localhost:3000/analyze \
  -H "Content-Type: application/json" \
  -d '{"prompt": "이 리뷰를 분석해줘: 맛은 좋은데 배달이 너무 느렸어요"}'
```

**서버 종료:** `Ctrl+C`

### 핵심 포인트
- 같은 서버에서 일반 CRUD와 AI 분석이 공존
- `/items` → database 노드로 라우팅, `/analyze` → llm_call 노드로 라우팅
- 라우팅은 http_server의 `output_port` 설정으로 결정

---

## 데모 7: 자율 AI 에이전트 — LLM이 도구를 스스로 선택 (API 키 필요!)

**이게 가장 중요한 킬러 데모입니다.**

**이게 뭐냐:** 사용자 질문을 받으면 AI가 **스스로** "검색", "계산", "대화" 중 어떤 도구를 쓸지 결정하는 자율형 에이전트.

**그래프 구조:**
```
http_server → memory → tool_router ─┬─ search → llm_call(검색) → console
                                     ├─ calculate → llm_call(계산) → console
                                     └─ respond → llm_call(대화) → console
```

### 실행 — 터미널 2개 필요!

**터미널 1:**

```bash
cargo run -p anp-cli -- run examples/ai_agent.anp.json
```

출력:
```
 INFO starting ANP engine name=Autonomous AI Agent
 INFO HTTP server listening port=3000
```

**터미널 2: 에이전트에게 다양한 질문 보내기**

```bash
# 질문 1: 수학 질문 → AI가 "calculate" 도구를 자동 선택
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is the square root of 144?"}'
# 서버 로그에 [agent] 접두사로 "12" 관련 답변이 나옴

# 질문 2: 일반 대화 → AI가 "respond" 도구를 자동 선택
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Tell me a joke"}'
# AI가 알아서 "respond" 경로로 라우팅

# 질문 3: 사실 질문 → AI가 "search" 도구를 자동 선택
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is the capital of France?"}'
# AI가 "search" 경로로 라우팅

# 질문 4: 복합 수학 → "calculate"
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Calculate 15% tip on a $85 dinner bill"}'

# 질문 5: 한국어도 됨!
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "대한민국의 수도는 어디인가요?"}'
```

**서버 종료:** `Ctrl+C`

### 핵심 포인트
- **if/else 없음. 라우팅 코드 없음.** AI가 판단함
- `tool_router` 노드가 LLM에게 "이 질문에 어떤 도구가 적합한지" 물어봄
- LLM이 `search`, `calculate`, `respond` 중 하나를 선택
- 선택된 도구에 해당하는 output port로 데이터가 흘러감
- `memory` 노드가 대화 히스토리를 유지 (같은 세션의 이전 대화를 기억)
- 서버 로그를 보면 어떤 노드를 거쳤는지 실시간으로 확인 가능

---

## 보너스: CLI 유틸리티 명령어들 (API 키 불필요)

### 그래프 검증 (Validate)

그래프가 올바른지 실행 없이 확인:

```bash
cargo run -p anp-cli -- validate examples/ai_agent.anp.json
```

출력:
```
Graph 'Autonomous AI Agent' is valid.
```

### 그래프 정보 (Info)

그래프의 노드/엣지 구조 요약:

```bash
cargo run -p anp-cli -- info examples/data_pipeline.anp.json
```

출력:
```
ANP Graph: AI Data Pipeline
Description: Reads data, fetches external API, enriches with AI, filters, and outputs...
Version: 0.1
Nodes: 5
Edges: 4

Nodes:
  [   source] source (type: file_read)
  [processor] parse (type: transform)
  [processor] high_score_filter (type: filter)
  [processor] format_report (type: template)
  [     sink] output (type: console)

Edges:
  e1 source.default -> parse.default
  e2 parse.default -> high_score_filter.default
  e3 high_score_filter.default -> format_report.default
  e4 format_report.default -> output.default
```

### 그래프 설명 (Explain)

사람이 읽을 수 있는 자연어 설명:

```bash
cargo run -p anp-cli -- explain examples/ai_agent.anp.json
```

출력:
```
This graph defines: Autonomous AI Agent
An AI agent that autonomously decides which tool to use...

  [service] HTTP server (port 3000) starts as a long-running service
  [process] 'memory' node transforms data
  [process] 'tool_router' node transforms data
  [process] 'llm_call' node transforms data
  [process] 'llm_call' node transforms data
  [process] 'llm_call' node transforms data
  [output] Console output consumes data

Data flow:
  server.agent -> memory
  memory -> router
  router.search -> search_handler
  router.calculate -> calc_handler
  router.respond -> chat_handler
  search_handler -> log
  calc_handler -> log
  chat_handler -> log
```

### 그래프 시각화 — GraphViz DOT 출력

```bash
cargo run -p anp-cli -- graph examples/ai_agent.anp.json
```

이 출력을 [GraphViz Online](https://dreampuf.github.io/GraphvizOnline/)에 붙여넣으면 그래프 이미지를 볼 수 있습니다.

또는 로컬에 graphviz가 설치되어 있다면:

```bash
cargo run -p anp-cli -- graph examples/ai_agent.anp.json > agent.dot
dot -Tpng agent.dot -o agent.png
open agent.png  # macOS
# 또는
xdg-open agent.png  # Linux
```

### 새 프로젝트 초기화 (Init)

```bash
cargo run -p anp-cli -- init my_project
```

출력:
```
Created ANP project 'my_project'

  cd my_project
  anp run main.anp.json
```

`my_project/` 디렉토리에 `main.anp.json`과 `data.txt`가 생성됩니다.

---

## 보너스: AI로 그래프 생성 & 편집 (API 키 필요!)

### 프롬프트로 그래프 자동 생성

```bash
# 그래프 생성 후 출력
cargo run -p anp-cli -- prompt "read data.txt and print it"

# 생성 + 즉시 실행
cargo run -p anp-cli -- prompt "read data.txt and print it" --run

# 생성 + 설명
cargo run -p anp-cli -- prompt "AI chatbot with memory" --explain

# 파일로 저장
cargo run -p anp-cli -- prompt "REST API that stores user data" --output my_api.anp.json
```

### 자연어로 그래프 편집

```bash
# 필터 조건 변경 (dry-run으로 미리보기)
cargo run -p anp-cli -- edit examples/data_pipeline.anp.json \
  "change the filter to keep scores above 90" --dry-run

# 실제 적용
cargo run -p anp-cli -- edit examples/data_pipeline.anp.json \
  "change the filter to keep scores above 90"
```

---

## 전체 테스트 실행 (개발자용)

프로젝트의 모든 테스트 확인:

```bash
# 전체 테스트 (111개)
cargo test

# Clippy 정적 분석 (경고 0개)
cargo clippy -- -D warnings
```

---

## 데모 순서 추천 (발표/영상용)

1. **Hello World** (10초) — "이게 ANP입니다. 코드 없이 JSON 그래프만으로."
2. **Data Pipeline** (20초) — "데이터 파이프라인도 JSON 하나로. 읽기→변환→필터→출력."
3. **REST API** (30초) — "HTTP 서버 + DB도 코드 없이." curl로 CRUD 시연.
4. **AI Agent** (60초) — **킬러 데모.** "AI가 스스로 도구를 선택합니다." 수학/검색/대화 질문을 던지며 자동 라우팅 시연.
5. **GraphViz** (10초) — "그래프 구조를 시각적으로 확인."
6. **Prompt** (20초) — "프롬프트 한 줄로 그래프를 생성."

**총 소요 시간: 약 2~3분**

---

## 문제 해결 (Troubleshooting)

### "command not found: cargo"
→ Rust가 설치되지 않음. `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` 실행.

### "No LLM API key found"
→ AI 관련 데모를 실행하려면 `export OPENAI_API_KEY=sk-...` 필요.

### "Address already in use (port 3000)"
→ 이전에 실행한 서버가 아직 살아있음. `Ctrl+C`로 종료하거나: `lsof -i :3000` → `kill <PID>`

### "connection refused" (curl 에러)
→ 서버가 아직 안 떴거나 다른 터미널에서 실행해야 함. 서버 로그에 "HTTP server listening"이 나올 때까지 기다린 후 curl.

### 빌드 에러
→ `cargo clean && cargo build`로 클린 빌드 시도.
