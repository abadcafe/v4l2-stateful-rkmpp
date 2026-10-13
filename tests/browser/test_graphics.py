"""module: tests.browser.test_graphics; responsibility: define the tested public contract.

operations: pytest cases; entities: Bounds, NativeWindow, Context; tests:
    test_browser_webgl_shaders_and_hardware_compositor
    test_browser_background_window_uses_desktop_and_hardware_compositor

Real GPU shader execution and compositing backend acceptance.
"""

import os
from pathlib import Path

from pydantic import BaseModel

from tests.browser.conftest import BrowserFactory
from tests.support.graphics import get_info
from tests.support.outcome import accepted


class Bounds(BaseModel):
    windowState: str


class NativeWindow(BaseModel):
    windowId: int
    bounds: Bounds


def test_browser_background_window_uses_desktop_and_hardware_compositor(
    browser_factory: BrowserFactory,
) -> None:
    browser = browser_factory(())
    assert browser.alive
    native = NativeWindow.model_validate(
        accepted(
            browser.control.call(
                "Browser.getWindowForTarget", {"targetId": browser.window.target}
            )
        ),
        strict=True,
    )
    assert native.windowId > 0 and native.bounds.windowState == "normal"
    assert accepted(browser.tab.evaluate("document.visibilityState", str)) == "visible"
    assert Path(f"/proc/{browser.pid}").stat().st_uid == os.geteuid()
    info = accepted(get_info(browser.control))
    assert info.skia and "Mali-G610" in info.renderer


class Context(BaseModel):
    api: str
    pixel: list[int]
    error: int
    lost: bool
    renderer: str


def test_browser_webgl_shaders_and_hardware_compositor(
    browser_factory: BrowserFactory,
) -> None:
    browser = browser_factory(())
    contexts = accepted(
        browser.tab.evaluate(
            r"""['webgl','webgl2'].map(api=>{
const canvas=document.createElement('canvas');canvas.width=canvas.height=4;
const gl=canvas.getContext(api,{preserveDrawingBuffer:true});
if(!gl)throw Error(api+' context unavailable');
const shader=(type,source)=>{const s=gl.createShader(type);gl.shaderSource(s,source);gl.compileShader(s);if(!gl.getShaderParameter(s,gl.COMPILE_STATUS))throw Error(gl.getShaderInfoLog(s));return s;};
const v2=api==='webgl2',p=gl.createProgram();
gl.attachShader(p,shader(gl.VERTEX_SHADER,v2?'#version 300 es\nin vec2 p;void main(){gl_Position=vec4(p,0.,1.);}':'attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}'));
gl.attachShader(p,shader(gl.FRAGMENT_SHADER,v2?'#version 300 es\nprecision mediump float;out vec4 c;void main(){c=vec4(.25,.5,.75,1.);}':'precision mediump float;void main(){gl_FragColor=vec4(.25,.5,.75,1.);}'));
gl.linkProgram(p);if(!gl.getProgramParameter(p,gl.LINK_STATUS))throw Error(gl.getProgramInfoLog(p));gl.useProgram(p);
const b=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,b);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,3,-1,-1,3]),gl.STATIC_DRAW);
const position=gl.getAttribLocation(p,'p');gl.enableVertexAttribArray(position);gl.vertexAttribPointer(position,2,gl.FLOAT,false,0,0);gl.viewport(0,0,4,4);gl.drawArrays(gl.TRIANGLES,0,3);
const pixel=new Uint8Array(4);gl.readPixels(2,2,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
const extension=gl.getExtension('WEBGL_debug_renderer_info');return {api,pixel:[...pixel],error:gl.getError(),lost:gl.isContextLost(),renderer:gl.getParameter(extension.UNMASKED_RENDERER_WEBGL)};
})""",
            list[Context],
        )
    )
    assert [context.api for context in contexts] == ["webgl", "webgl2"]
    for context in contexts:
        assert context.error == 0 and not context.lost
        assert "Mali-G610" in context.renderer
        assert all(
            abs(actual - expected) <= 1
            for actual, expected in zip(context.pixel, [64, 128, 191, 255], strict=True)
        )
    info = accepted(get_info(browser.control))
    assert info.skia and "Mali-G610" in info.renderer
