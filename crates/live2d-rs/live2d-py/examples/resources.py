import os
import sys

CURRENT_DIRECTORY = os.path.split(__file__)[0]
RESOURCES_DIRECTORY = os.path.join(CURRENT_DIRECTORY, "..", "Resources")

_package_dir = os.path.join(CURRENT_DIRECTORY, "..", "package")                                                                                  
if _package_dir not in sys.path:                                                                                                                 
    sys.path.insert(0, os.path.abspath(_package_dir)) 