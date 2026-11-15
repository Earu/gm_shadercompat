#include "shim.h"

size_t g_ToglTruncations = 0;

#define private public
#define protected public
#include "dx9asmtogl2.cpp"
#undef private
#undef protected

static const int MAX_TRANSLATION_SIZE = 500000;

extern "C" int gmsc_translate(const uint32_t *code, uint32_t options, int32_t shadowDepthSamplerMask, uint32_t centroidMask, char *glsl, size_t glslCap, char *err, size_t errCap, size_t *errToken, size_t *truncations)
{
	D3DToGL translator;
	CUtlBuffer out(3000, MAX_TRANSLATION_SIZE, CUtlBuffer::TEXT_BUFFER);
	bool isVertexShader = false;
	char label[] = "gm_shadercompat";

	g_ToglTruncations = 0;
	translator.m_pdwBaseToken = (uint32 *)code;
	translator.m_pdwNextToken = (uint32 *)code;

	int status = 0;
	try
	{
		translator.TranslateShader((uint32 *)code, &out, &isVertexShader, options, shadowDepthSamplerMask, centroidMask, label);
	}
	catch (const std::exception &e)
	{
		V_strncpy(err, e.what(), (int)errCap);
		*errToken = (size_t)(translator.m_pdwNextToken - (uint32 *)code);
		status = 1;
	}

	*truncations = g_ToglTruncations;
	V_strncpy(glsl, (const char *)out.Base(), (int)glslCap);
	return status;
}
