-- BMake secret.bm.locksys previewer for Yazi.
-- Registration (in ~/.config/yazi/yazi.toml):
--
--   [[plugin.prepend_previewers]]
--   name = "*.locksys"
--   run = "locksys"
--
-- Property by ZoderStudio org

local M = {}

function M:peek()
	local lines = {
		ui.Line("this file is securely protected and cannot open"),
		ui.Line("-----------------------------------------------------------------------"),
		ui.Line(""),
		ui.Line("Property by ZoderStudio org"),
	}
	ya.preview_widget(self, ui.Text(lines):area(self.area):wrap(ui.Text.WRAP))
end

function M:seek() end

return M