from ipywidgets import DOMWidget
from traitlets import Unicode
from vegafusion import PyTaskGraphRuntime

from ._frontend import module_name, module_version
import altair as alt

from .runtime import runtime


class VegaFusionWidget(DOMWidget):
    _model_name = Unicode('VegaFusionModel').tag(sync=True)
    _model_module = Unicode(module_name).tag(sync=True)
    _model_module_version = Unicode(module_version).tag(sync=True)
    _view_name = Unicode('VegaFusionView').tag(sync=True)
    _view_module = Unicode(module_name).tag(sync=True)
    _view_module_version = Unicode(module_version).tag(sync=True)
    vegalite_spec = Unicode(None, allow_none=True).tag(sync=True)
    vega_spec_full = Unicode(None, allow_none=True, read_only=True).tag(sync=True)

    # vegalite_spec
    # vegaspec_full
    # vegaspec_client
    # vegaspec_server
    #
    # server_to
    def __init__(self, *args, **kwargs):

        # Support altair object as single positional argument
        if len(args) == 1:
            chart = args[0]
            vegalite_spec = chart.to_json()
            kwargs["vegalite_spec"] = vegalite_spec

        super().__init__(**kwargs)

        # Wire up widget message callback
        self.on_msg(self._handle_message)

    def _handle_message(self, widget, msg, buffers):
        # print(msg)
        if msg['type'] == "request":
            # print("py: handle request")
            # Build response
            response_bytes = runtime.process_request_bytes(
                buffers[0]
            )
            # print("py: send response")
            self.send(dict(type="response"), [response_bytes])


def vegafusion_renderer(spec):
    import json
    from IPython.display import display

    # Display widget as a side effect, then return empty string text representation
    # so that Altair doesn't also display a string representation
    widget = VegaFusionWidget(vegalite_spec=json.dumps(spec))
    display(widget)
    return {'text/plain': ""}


alt.renderers.register('vegafusion', vegafusion_renderer)
