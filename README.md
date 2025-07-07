# Telegram Reaction Bot System

Система из двух ботов для автоматических реакций в Telegram чатах.

## Архитектура

### 1. **telegram-reaction-bot** - Основной бот реакций
- Мониторит чаты в реальном времени
- Автоматически ставит 👍 на сообщения с ценами выше заданного порога
- Поддерживает фильтры по банку, реквизитам и сумме
- Использует TDLib для максимальной скорости (<1мс)

### 2. **telegram-likes-manager-bot** - Контрольный бот
- Управляет основным ботом через Telegram команды
- Позволяет настраивать фильтры без перезапуска
- Контролирует доступ пользователей

## Быстрый старт на сервере

### 1. Подготовка

```bash
# Клонируйте репозиторий
git clone <your-repo-url>
cd telegram-bot-system

# Скопируйте пример конфигурации
cp env.example .env
```

### 2. Настройка конфигурации

Отредактируйте файл `.env`:

```bash
nano .env
```

**ОБЯЗАТЕЛЬНО измените:**

```env
# Получите на https://my.telegram.org/apps
TELEGRAM_API_ID=12345678
TELEGRAM_API_HASH=abcdef1234567890abcdef1234567890

# Получите от @BotFather
BOT_TOKEN=1234567890:ABCdefGHIjklMNOpqrsTUVwxyz

# Ваши ID пользователей (через запятую)
ALLOWED_USERS=123456789,987654321

# ID чатов для мониторинга (через запятую)
ALLOWED_CHAT_IDS=-1002685602852,-4649902952
```

### 3. Запуск через Docker

```bash
# Собрать и запустить
docker-compose up -d

# Посмотреть логи
docker-compose logs -f

# Остановить
docker-compose down
```

### 4. Проверка работы

1. Найдите вашего контрольного бота в Telegram
2. Отправьте команду `/start`
3. Настройте фильтры через команды:
   - `/bank t` - фильтр по T-Bank
   - `/requisite +` - фильтр по СБП
   - `/amount 50000` - минимальная сумма
   - `/status` - проверить статус

## Команды управления

### Основные команды
- `/start` - запустить бот реакций
- `/stop` - остановить бот реакций
- `/status` - проверить статус

### Настройка фильтров
- `/bank t` - фильтр по банку (например, "t" для T-Bank)
- `/requisite +` - фильтр по реквизитам (например, "+" для СБП)
- `/amount 50000` - минимальная сумма для реакции
- `/clear` - очистить все фильтры

## Ручная установка (без Docker)

### Требования
- Rust 1.75+
- TDLib 1.8+
- Linux/macOS (Windows не тестировался)

### Установка TDLib

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install build-essential cmake git zlib1g-dev libssl-dev gperf php-cli
git clone https://github.com/tdlib/td.git
cd td
mkdir build && cd build
cmake -DCMAKE_BUILD_TYPE=Release ..
cmake --build . --target install
```

**macOS:**
```bash
brew install tdlib
```

### Сборка проектов

```bash
# Основной бот реакций
cd telegram-reaction-bot
cargo build --release

# Контрольный бот
cd ../telegram-likes-manager-bot
cargo build --release
```

### Запуск

```bash
# Терминал 1: Основной бот
cd telegram-reaction-bot
cp env.example .env
# Отредактируйте .env
cargo run --release

# Терминал 2: Контрольный бот
cd telegram-likes-manager-bot
cp env.example .env
# Отредактируйте .env
cargo run --release
```

## Настройка на продакшене

### 1. Системный сервис (systemd)

Создайте файл `/etc/systemd/system/telegram-bots.service`:

```ini
[Unit]
Description=Telegram Reaction Bots
After=network.target

[Service]
Type=simple
User=telegram-bot
WorkingDirectory=/opt/telegram-bots
ExecStart=/usr/bin/docker-compose up
ExecStop=/usr/bin/docker-compose down
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

### 2. Автозапуск

```bash
sudo systemctl enable telegram-bots
sudo systemctl start telegram-bots
sudo systemctl status telegram-bots
```

### 3. Мониторинг

```bash
# Логи в реальном времени
docker-compose logs -f

# Статистика контейнеров
docker stats

# Проверка статуса
docker-compose ps
```

## Безопасность

### 1. Ограничение доступа
- Настройте `ALLOWED_USERS` только для доверенных пользователей
- Используйте отдельного пользователя для запуска ботов

### 2. Сетевая безопасность
- Боты работают только исходящие соединения
- Не требуется открывать порты

### 3. Обновления
```bash
# Обновить код
git pull

# Пересобрать и перезапустить
docker-compose down
docker-compose up -d --build
```

## Устранение неполадок

### Проблемы с авторизацией
1. Проверьте правильность `TELEGRAM_API_ID` и `TELEGRAM_API_HASH`
2. Убедитесь, что номер телефона введен с кодом страны
3. Проверьте 2FA пароль, если включен

### Бот не реагирует
1. Проверьте `ALLOWED_CHAT_IDS` - бот должен быть участником чата
2. Убедитесь, что бот имеет права на реакции в чате
3. Проверьте фильтры через `/status`

### Ошибки сборки
1. Обновите Rust: `rustup update`
2. Очистите кэш: `cargo clean`
3. Проверьте зависимости TDLib

## Поддержка

При возникновении проблем:
1. Проверьте логи: `docker-compose logs -f`
2. Убедитесь в правильности конфигурации
3. Проверьте права доступа к чатам

## Лицензия

MIT License 