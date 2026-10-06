#!/usr/bin/env python3
"""Self-authored layer-transport fixtures, independent of Emulsion export policy.

Requires the existing psd-tools 1.23.0 and Pillow. These are not Photoshop
rendering references. All source pixels are solid, synthetic colors below.
"""
from pathlib import Path
import hashlib,json
from PIL import Image
import psd_tools
from psd_tools import PSDImage
from psd_tools.api.layers import PixelLayer
from psd_tools.constants import BlendMode,Resource

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'crates/emulsion-io/tests/fixtures/psd/storyboard'
CASES=[
 ('layout.psd',(128,72),[
  ('Figure',(231,124,89,255),BlendMode.NORMAL,False),
  ('Shadow',(124,124,170,255),BlendMode.MULTIPLY,True)]),
 ('key-art.psd',(40,20),[
  ('Paper',(243,243,237,255),BlendMode.NORMAL,False),
  ('Shade',(113,113,188,153),BlendMode.MULTIPLY,False),
  ('Light',(231,218,170,128),BlendMode.SCREEN,True)])]

def main():
 assert psd_tools.__version__=='1.23.0'
 OUT.mkdir(parents=True,exist_ok=True)
 result=[]
 for name,size,layers in CASES:
  psd=PSDImage.new('RGBA',size)
  # Optional VersionInfo is absent: the psd-tools preview is not an Adobe
  # compatibility/gamma oracle. Tests exercise transport of editable layers.
  del psd.image_resources[Resource.VERSION_INFO]
  for label,rgba,blend,clip in layers:
   node=PixelLayer.frompil(Image.new('RGBA',size,rgba),psd,name=label)
   node.blend_mode=blend;node.clipping=clip
  path=OUT/name;psd.save(path)
  back=PSDImage.open(path)
  assert [(l.name,l.blend_mode,l.clipping) for l in back]==[(n,b,c) for n,_,b,c in layers]
  for layer,(_,rgba,_,_) in zip(back,layers):
   assert layer.topil(apply_icc=False).getpixel((0,0))==rgba
  result.append({'file':name,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'size':size,
                 'layers':[{'name':n,'rgba8':rgba,'blend':b.name,'clipped':c} for n,rgba,b,c in layers]})
 (OUT/'manifest.json').write_text(json.dumps({'producer':'psd-tools 1.23.0','authorship':'Self-authored synthetic Emulsion test artwork; no Photoshop application used','cases':result},indent=2)+'\n')
 print(json.dumps(result,indent=2))
if __name__=='__main__':main()
