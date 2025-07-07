# Миграция на сервер

## Что было исправлено

### 1. **Добавлены API_ID и API_HASH**
- Раньше: Жестко заданные в коде
- Теперь: Читаются из переменных окружения

### 2. **Настраиваемые пользователи**
- Раньше: Жестко заданные в коде
- Теперь: `ALLOWED_USERS` в .env файле

### 3. **Настраиваемые чаты**
- Раньше: `["-1002685602852", "-4649902952"]`
- Теперь: `ALLOWED_CHAT_IDS` в .env файле

### 4. **Docker поддержка**
- Полная контейнеризация для легкого развертывания
- Автоматическая сборка TDLib
- Персистентные данные между перезапусками

## Пошаговая миграция

### 1. Подготовка сервера

```bash
# Установка Docker (Ubuntu/Debian)
sudo apt update
sudo apt install docker.io docker-compose

# Создание пользователя для ботов
sudo useradd -m -s /bin/bash telegram-bot
sudo usermod -aG docker telegram-bot
```

### 2. Загрузка кода

```bash
# Клонирование на сервер
git clone <your-repo> /opt/telegram-bots
cd /opt/telegram-bots
sudo chown -R telegram-bot:telegram-bot /opt/telegram-bots
```

### 3. Настройка конфигурации

```bash
# Переключение на пользователя бота
sudo -u telegram-bot bash

# Создание конфигурации
cp env.example .env
nano .env
```

**Обязательные настройки в .env:**

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

### 4. Запуск

```bash
# Автоматическое развертывание
./deploy.sh

# Или вручную
docker-compose up -d
```

### 5. Проверка

```bash
# Статус контейнеров
docker-compose ps

# Логи
docker-compose logs -f

# Тест бота в Telegram
# Найдите бота и отправьте /start
```

## Изменение пользователей и чатов

### Добавить нового пользователя:
```env
ALLOWED_USERS=123456789,987654321,555666777
```

### Добавить новый чат:
```env
ALLOWED_CHAT_IDS=-1002685602852,-4649902952,-1001234567890
```

### Применить изменения:
```bash
docker-compose restart manager-bot
```

## Резервное копирование

### Данные для бэкапа:
```bash
# Конфигурация
cp .env /backup/telegram-bots.env

# TDLib данные (сессии)
docker cp telegram-reaction-bot:/app/tdlib_data /backup/
docker cp telegram-reaction-bot:/app/tdlib_files /backup/
```

### Восстановление:
```bash
# Конфигурация
cp /backup/telegram-bots.env .env

# TDLib данные
docker cp /backup/tdlib_data telegram-reaction-bot:/app/
docker cp /backup/tdlib_files telegram-reaction-bot:/app/

# Перезапуск
docker-compose restart
```

## Мониторинг

### Системный сервис:
```bash
# Создание systemd сервиса
sudo tee /etc/systemd/system/telegram-bots.service << EOF
[Unit]
Description=Telegram Reaction Bots
After=docker.service
Requires=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/opt/telegram-bots
ExecStart=/usr/bin/docker-compose up -d
ExecStop=/usr/bin/docker-compose down
User=telegram-bot
Group=telegram-bot

[Install]
WantedBy=multi-user.target
EOF

# Включение автозапуска
sudo systemctl enable telegram-bots
sudo systemctl start telegram-bots
```

### Логирование:
```bash
# Настройка ротации логов
sudo tee /etc/logrotate.d/telegram-bots << EOF
/opt/telegram-bots/logs/*.log {
    daily
    missingok
    rotate 7
    compress
    delaycompress
    notifempty
    create 644 telegram-bot telegram-bot
}
EOF
```

## Безопасность

### Ограничение доступа:
```bash
# Только SSH доступ к серверу
# Боты работают только исходящие соединения
# Не требуется открывать порты

# Ограничение прав пользователя
sudo chmod 600 /opt/telegram-bots/.env
sudo chown telegram-bot:telegram-bot /opt/telegram-bots/.env
```

### Обновления:
```bash
# Автоматическое обновление
cd /opt/telegram-bots
git pull
docker-compose down
docker-compose up -d --build
```

## Устранение проблем

### Бот не авторизуется:
1. Проверьте API_ID и API_HASH
2. Убедитесь в правильности номера телефона
3. Проверьте 2FA пароль

### Бот не реагирует:
1. Проверьте ALLOWED_CHAT_IDS
2. Убедитесь, что бот в чате
3. Проверьте права на реакции

### Ошибки Docker:
```bash
# Очистка и пересборка
docker-compose down --volumes --remove-orphans
docker system prune -f
./deploy.sh --clean
``` 