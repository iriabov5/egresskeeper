# Единые команды разработки.
#
# `make check` запускает формат, линтеры, тесты backend и frontend и сборку.

CARGO ?= cargo
PNPM ?= pnpm
SONAR_HOST ?= http://localhost:9000
SONAR_TOKEN ?=

.PHONY: help check fmt lint test test-rust test-web build dev sonar-clippy sonar-coverage sonar

help:
	@echo "make check      — все проверки: формат, линтеры, тесты, сборка"
	@echo "make fmt        — форматирование Rust"
	@echo "make lint       — clippy и eslint"
	@echo "make test       — тесты Rust и frontend"
	@echo "make build      — сборка frontend и приложения"
	@echo "make dev        — запуск приложения в dev-режиме"

check: fmt lint test build

fmt:
	$(CARGO) fmt --all --check

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(PNPM) lint

test: test-rust test-web

test-rust:
	$(CARGO) test --workspace

test-web:
	$(PNPM) test

build:
	$(PNPM) build
	$(CARGO) build --workspace

dev:
	$(PNPM) tauri dev

# Статический анализ. Требуется запущенный SonarQube и токен:
#   docker run -d --name sonarqube -p 9000:9000 sonarqube:community
#   SONAR_TOKEN=<токен> make sonar
sonar-clippy:
	@mkdir -p target/sonar
	$(CARGO) clippy --workspace --all-targets --message-format=json > target/sonar/clippy-report.json

# Отчёты о покрытии для статического анализа.
sonar-coverage:
	@mkdir -p target/sonar
	$(CARGO) llvm-cov --workspace --lcov --output-path target/sonar/rust-lcov.info
	$(PNPM) test:coverage

sonar: sonar-clippy sonar-coverage
	@if [ -z "$(SONAR_TOKEN)" ]; then echo "SONAR_TOKEN не задан"; exit 1; fi
	docker run --rm \
		-e SONAR_HOST_URL=http://host.docker.internal:9000 \
		-e SONAR_TOKEN=$(SONAR_TOKEN) \
		-v "$(PWD):$(PWD)" -w "$(PWD)" \
		sonarsource/sonar-scanner-cli:latest \
		-Dsonar.host.url=http://host.docker.internal:9000 \
		-Dsonar.projectBaseDir=$(PWD)
