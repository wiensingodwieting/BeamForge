#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"

echo "=== 1. Проверка файлов ==="
git status -s

echo "=== 2. Коммит изменений ==="
git add -A
git commit -m "Release v0.3.0: Streaming previews, real-time conflicts, UI alignment & icon overhaul" || echo "No changes to commit"

echo "=== 3. Создание тега v0.3.0 ==="
git tag -d v0.3.0 2>/dev/null || true
git tag -a v0.3.0 -m "BeamForge v0.3.0"

echo "=== 4. Пуш в репозиторий GitHub ==="
git push origin main
git push origin v0.3.0 --force

echo ""
echo "=== Готово! GitHub Actions запустил сборку релиза v0.3.0 ==="
echo "Следить за процессом: https://github.com/wiensingodwieting/BeamForge/actions"
