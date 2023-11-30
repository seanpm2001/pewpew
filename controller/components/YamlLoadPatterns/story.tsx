import { DisplayDivBody, DisplayDivMain } from "../YamlWriterForm";
import { LOAD_PATTERN, LoadPatternProps, LoadPatterns, RAMP_PATTERN } from ".";
import { GlobalStyle } from "../Layout";
import { PewPewLoadPattern } from "../../types";
import React from "react";

const props: LoadPatternProps = {
  addPattern: (pewpewPattern: PewPewLoadPattern) => {
    // eslint-disable-next-line no-console
    console.log("Adding new LoadPattern", pewpewPattern);
  },
  deletePattern: (id: string) => {
    // eslint-disable-next-line no-console
    console.log("Removing LoadPattern " + id);
  },
  clearAllPatterns: () => {
    // eslint-disable-next-line no-console
    console.log("Removing all LoadPatterns");
  },
  changePattern: (pewpewPattern: PewPewLoadPattern) => {
    // eslint-disable-next-line no-console
    console.log("changing LoadPattern " + pewpewPattern.id, pewpewPattern);
  },
  defaultYaml: false,
  patterns: []
};

const propsDefault: LoadPatternProps = {
  ...props,
  defaultYaml: true,
  patterns: [
    { id: RAMP_PATTERN, from: "10", to: "100", over: "15m" },
    { id: LOAD_PATTERN, from: "100", to: "100", over: "15m" }
  ]
};

const propsLoaded: LoadPatternProps = {
  ...props,
  defaultYaml: false,
  patterns: [
    { id: "0", from: "10", to: "100", over: "15m" },
    { id: "1", from: "100", to: "100", over: "15m" },
    { id: "2", from: "", to: "", over: "" },
    { id: "3", from: "", to: "", over: "15m" },
    { id: "4", from: "10", to: "", over: "" },
    { id: "5", from: "", to: "100", over: "" },
    { id: "6", from: "", to: "100", over: "15m" },
    { id: "7", from: "10", to: "100", over: "5m" }
  ]
};

export default {
  title: "YamlLoadPatterns"
};

export const Default = () => (
  <React.Fragment>
    <GlobalStyle />
    <DisplayDivMain>
      <DisplayDivBody>
        <LoadPatterns {...propsDefault}></LoadPatterns>
      </DisplayDivBody>
    </DisplayDivMain>
  </React.Fragment>
);

export const Empty = () => (
  <React.Fragment>
    <GlobalStyle />
    <DisplayDivMain>
      <DisplayDivBody>
        <LoadPatterns {...props}></LoadPatterns>
      </DisplayDivBody>
    </DisplayDivMain>
  </React.Fragment>
);

export const Loaded = () => (
  <React.Fragment>
    <GlobalStyle />
    <DisplayDivMain>
      <DisplayDivBody>
        <LoadPatterns {...propsLoaded}></LoadPatterns>
      </DisplayDivBody>
    </DisplayDivMain>
  </React.Fragment>
);
