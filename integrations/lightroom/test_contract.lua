-- Mocked SDK contract: no host catalog or real photos are touched.
local files, directories, messages, sessions = {}, {}, {}, 0
local collection = { getName = function() return 'Travel "2026"' end }
local photo = {
    getRawMetadata = function(_, key) return ({path='/source/a.NEF', rating=5, pickStatus=1})[key] end,
    getDevelopSettings = function() return {Exposure2012=0.5, ToneCurvePV2012Red={0,0,128,160,255,255}} end,
    getContainedCollections = function() return {collection} end,
}
local modules = {
    LrApplication = {activeCatalog=function() return {getTargetPhotos=function() return {photo} end} end},
    LrDialogs = {
        runOpenPanel=function() return {'/handoffs'} end,
        message=function(title, body, kind) messages[#messages+1]={title,body,kind} end,
    },
    LrTasks={startAsyncTask=function(fn) fn() end, pcall=pcall},
    LrPathUtils={child=function(a,b) return a..'/'..b end, leafName=function(s) return s:match('[^/]+$') end},
    LrFileUtils={
        exists=function(p) return directories[p] end,
        createAllDirectories=function(p) directories[p]=true end,
        copy=function(source,dest) assert(source=='/source/a.NEF'); files[dest]='original'; return true end,
    },
    LrExportSession=function(args)
        sessions=sessions+1
        local s=args.exportSettings
        assert(s.LR_format=='TIFF' and s.LR_tiff_bitDepth==16)
        assert(s.LR_export_colorSpace=='sRGB' and not s.LR_size_doConstrain)
        return {renditions=function()
            local used=false
            return function()
                if used then return nil end
                used=true
                return 1,{waitForRender=function() return true,s.LR_export_destinationPathPrefix..'/1.TIF' end}
            end
        end}
    end,
}
_G.import=function(name) return assert(modules[name],name) end
local original_open=io.open
io.open=function(path,mode)
    assert(mode=='wb' and path:match('/handoff%.emulr%.json$'))
    return {write=function(_,bytes) files[path]=bytes; return true end,close=function() return true end}
end
dofile('integrations/lightroom/Emulsion.lrplugin/Export.lua')
io.open=original_open
assert(sessions==1 and #messages==1 and messages[1][3]=='info')
local manifest
for path,bytes in pairs(files) do if path:match('%.json$') then manifest=bytes end end
assert(manifest and manifest:find('"format":"emulsion%-lightroom%-handoff"'))
assert(manifest:find('"rendered":"rendered/1.TIF"',1,true))
assert(manifest:find('Travel \\"2026\\"',1,true))
assert(manifest:find('"ToneCurvePV2012Red":[0,0,128,160,255,255]',1,true))
print('Catalog companion SDK contract passed (mock host)')
