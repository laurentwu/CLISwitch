ALTER TABLE app_settings
ADD COLUMN ui_zoom_percent INTEGER NOT NULL DEFAULT 100
CHECK (ui_zoom_percent IN (100, 125, 150, 175, 200, 225, 250, 275, 300));
