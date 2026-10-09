-- Runs inside the host photo catalog application. No proprietary vendor profile data is distributed.
local LrApplication = import 'LrApplication'
local LrDialogs = import 'LrDialogs'
local LrTasks = import 'LrTasks'
local LrPathUtils = import 'LrPathUtils'
local LrFileUtils = import 'LrFileUtils'
local LrExportSession = import 'LrExportSession'
local function quote(value)
    return '"' .. value:gsub('[%z\1-\31\\"]', function(c)
        if c == '"' then return '\\"' end
        if c == '\\' then return '\\\\' end
        return string.format('\\u%04x', string.byte(c))
    end) .. '"'
end
local function json(v, depth)
    depth = (depth or 0) + 1
    assert(depth <= 32, 'Settings nesting is too deep')
    if type(v) == 'string' then return quote(v) end
    if type(v) == 'boolean' then return v and 'true' or 'false' end
    if type(v) == 'number' then assert(v == v and math.abs(v) < math.huge, 'Nonfinite setting'); return tostring(v) end
    if type(v) ~= 'table' then return 'null' end
    local count, array = 0, true
    for k in pairs(v) do count = count + 1; if type(k) ~= 'number' or k < 1 or k % 1 ~= 0 then array = false end end
    if count ~= #v then array = false end
    local parts = {}
    if array then
        for i = 1, #v do parts[#parts + 1] = json(v[i], depth) end
        return '[' .. table.concat(parts, ',') .. ']'
    end
    for k, value in pairs(v) do if type(k) == 'string' then parts[#parts + 1] = quote(k) .. ':' .. json(value, depth) end end
    table.sort(parts)
    return '{' .. table.concat(parts, ',') .. '}'
end
LrTasks.startAsyncTask(function()
    local ok, message = LrTasks.pcall(function()
        local photos = LrApplication.activeCatalog():getTargetPhotos()
        assert(#photos > 0, 'Select photos first')
        local choice = LrDialogs.runOpenPanel { title = 'Choose parent folder for a new Emulsion handoff', canChooseFiles = false, canChooseDirectories = true, allowsMultipleSelection = false }
        if not choice then return end
        local root = LrPathUtils.child(choice[1], 'Emulsion-handoff-' .. os.date('%Y%m%d-%H%M%S'))
        assert(not LrFileUtils.exists(root), 'This handoff folder already exists; retry in a moment')
        LrFileUtils.createAllDirectories(LrPathUtils.child(root, 'originals'))
        LrFileUtils.createAllDirectories(LrPathUtils.child(root, 'rendered'))
        local bundle = { format = 'emulsion-lightroom-handoff', version = 1, photos = {} }
        for index, photo in ipairs(photos) do
            local source = photo:getRawMetadata('path')
            local relative = 'originals/' .. index .. '-' .. LrPathUtils.leafName(source)
            assert(LrFileUtils.copy(source, LrPathUtils.child(root, relative)), 'Could not copy original')
            local record = { original = relative, settings = photo:getDevelopSettings(), rating = photo:getRawMetadata('rating') or 0, flag = photo:getRawMetadata('pickStatus') or 0, collections = {} }
            for _, collection in ipairs(photo:getContainedCollections()) do record.collections[#record.collections + 1] = collection:getName() end
            local session = LrExportSession {
                photosToExport = { photo },
                exportSettings = {
                    LR_export_destinationType = 'specificFolder',
                    LR_export_destinationPathPrefix = LrPathUtils.child(root, 'rendered'),
                    LR_export_useSubfolder = false, LR_collisionHandling = 'rename',
                    LR_format = 'TIFF', LR_tiff_bitDepth = 16, LR_tiff_compressionMethod = 'compressionMethod_ZIP',
                    LR_export_colorSpace = 'sRGB', LR_size_doConstrain = false,
                    LR_outputSharpeningOn = false, LR_reimportExportedPhoto = false,
                    LR_renamingTokensOn = true, LR_tokens = '{{custom_token}}', LR_tokenCustomString = tostring(index),
                }
            }
            for _, rendition in session:renditions { stopIfCanceled = true } do
                local success, rendered = rendition:waitForRender()
                assert(success, rendered)
                record.rendered = 'rendered/' .. LrPathUtils.leafName(rendered)
            end
            assert(record.rendered, 'Rendering was canceled; handoff is incomplete')
            bundle.photos[#bundle.photos + 1] = record
        end
        local file = assert(io.open(LrPathUtils.child(root, 'handoff.emulr.json'), 'wb'))
        assert(file:write(json(bundle))); assert(file:close())
        LrDialogs.message('Emulsion handoff ready', 'Import handoff.emulr.json in Emulsion Library. Originals, translated settings, collections and 16-bit rendered references are included.\n' .. root, 'info')
    end)
    if not ok then LrDialogs.message('Emulsion handoff failed', tostring(message), 'critical') end
end)
