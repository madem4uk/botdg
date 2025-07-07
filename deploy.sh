#!/bin/bash

# Telegram Bot System Deployment Script
# Автоматическое развертывание системы ботов на сервере

set -e

echo "🚀 Запуск развертывания Telegram Bot System..."

# Проверка наличия Docker
if ! command -v docker &> /dev/null; then
    echo "❌ Docker не установлен. Установите Docker и попробуйте снова."
    exit 1
fi

if ! command -v docker-compose &> /dev/null; then
    echo "❌ Docker Compose не установлен. Установите Docker Compose и попробуйте снова."
    exit 1
fi

# Проверка наличия .env файла
if [ ! -f ".env" ]; then
    echo "📝 Создание файла конфигурации..."
    if [ -f "env.example" ]; then
        cp env.example .env
        echo "✅ Файл .env создан из примера"
        echo "⚠️  ОБЯЗАТЕЛЬНО отредактируйте .env файл перед запуском!"
        echo "   nano .env"
        exit 1
    else
        echo "❌ Файл env.example не найден"
        exit 1
    fi
fi

# Проверка обязательных переменных
echo "🔍 Проверка конфигурации..."

source .env

if [ "$TELEGRAM_API_ID" = "your_api_id_here" ] || [ -z "$TELEGRAM_API_ID" ]; then
    echo "❌ TELEGRAM_API_ID не настроен в .env файле"
    exit 1
fi

if [ "$TELEGRAM_API_HASH" = "your_api_hash_here" ] || [ -z "$TELEGRAM_API_HASH" ]; then
    echo "❌ TELEGRAM_API_HASH не настроен в .env файле"
    exit 1
fi

if [ "$BOT_TOKEN" = "your_bot_token_here" ] || [ -z "$BOT_TOKEN" ]; then
    echo "❌ BOT_TOKEN не настроен в .env файле"
    exit 1
fi

if [ -z "$ALLOWED_USERS" ]; then
    echo "❌ ALLOWED_USERS не настроен в .env файле"
    exit 1
fi

if [ -z "$ALLOWED_CHAT_IDS" ]; then
    echo "❌ ALLOWED_CHAT_IDS не настроен в .env файле"
    exit 1
fi

echo "✅ Конфигурация проверена"

# Остановка существующих контейнеров
echo "🛑 Остановка существующих контейнеров..."
docker-compose down 2>/dev/null || true

# Удаление старых образов (опционально)
if [ "$1" = "--clean" ]; then
    echo "🧹 Очистка старых образов..."
    docker-compose down --rmi all 2>/dev/null || true
fi

# Сборка и запуск
echo "🔨 Сборка контейнеров..."
docker-compose build --no-cache

echo "🚀 Запуск сервисов..."
docker-compose up -d

# Проверка статуса
echo "⏳ Ожидание запуска сервисов..."
sleep 10

echo "📊 Статус сервисов:"
docker-compose ps

echo ""
echo "✅ Развертывание завершено!"
echo ""
echo "📋 Полезные команды:"
echo "   docker-compose logs -f          # Просмотр логов"
echo "   docker-compose ps               # Статус контейнеров"
echo "   docker-compose down             # Остановка"
echo "   docker-compose restart          # Перезапуск"
echo ""
echo "🤖 Найдите вашего бота в Telegram и отправьте /start"
echo "📖 Подробная документация в README.md" 